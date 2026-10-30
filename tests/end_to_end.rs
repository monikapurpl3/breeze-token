//! breeze-token end to end, with everything real but Midea: breeze-mock's
//! units, Breeze Core 4.3.2 and Breeze μcore as the servers, and a pretend
//! MSmartHome account (the `fake-cloud` feature) that holds the units'
//! tokens.
//!
//! The units have to be on a private network of their own (discovery and the
//! servers' probes refuse anything else), so this needs breeze-mock's dummy
//! segment: Linux with CAP_NET_ADMIN, a container being enough.
//!
//!   cargo zigbuild --release --target x86_64-unknown-linux-musl --features fake-cloud --tests
//!   BREEZE_DUMMY_SEGMENTS=1 BREEZE_TOKEN_BIN=... BREEZE_CORE_BIN=... BREEZE_MOCK_BIN=... UCORE_BIN=... \
//!     ./end_to_end-<hash>
//!
//! Skipped otherwise.

use serde_json::{json, Value};
use sha2::Digest;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const KEY: &str = "e2e-api-key-not-a-secret";
const TOKEN: &str = "e2e-device-token-not-a-secret-0123456789";
const ACCOUNT: &str = "owner@example.com";
const PASSWORD: &str = "correct horse";

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Env {
    token_bin: PathBuf,
    core_bin: PathBuf,
    ucore_bin: PathBuf,
    work: PathBuf,
    cloud: PathBuf,
    /// Every token and key, none of which may ever be printed.
    secrets: Vec<String>,
    alpha: Value,
    bravo: Value,
}

fn call(port: u16, method: &str, path: &str, body: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nX-API-Key: {KEY}\r\nAuthorization: Bearer {TOKEN}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut r = BufReader::new(s);
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    let status = line.split(' ').nth(1).unwrap().parse().unwrap();
    loop {
        line.clear();
        r.read_line(&mut line).unwrap();
        if line.trim_end().is_empty() {
            break;
        }
    }
    let mut body = Vec::new();
    r.read_to_end(&mut body).unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn wait_for(addr: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while TcpStream::connect(addr).is_err() {
        assert!(Instant::now() < deadline, "nothing on {addr}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Env {
    /// Run breeze-token; the password goes in on standard input.
    fn token(&self, args: &[&str], password: &str, api_key: Option<&str>) -> (i32, String) {
        let mut cmd = Command::new(&self.token_bin);
        cmd.args(["--cloud", "smarthome", "--account", ACCOUNT, "--password-stdin"])
            .args(args)
            .env("BREEZE_TOKEN_FAKE_CLOUD", &self.cloud)
            .env_remove("BREEZE_API_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(k) = api_key {
            cmd.env("BREEZE_API_KEY", k);
        }
        let mut child = cmd.spawn().unwrap();
        writeln!(child.stdin.take().unwrap(), "{password}").unwrap();
        let out = child.wait_with_output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        for s in &self.secrets {
            assert!(!text.to_ascii_lowercase().contains(&s.to_ascii_lowercase()), "a token or key was printed:\n{text}");
        }
        assert!(!text.contains(PASSWORD), "the password was printed");
        (out.status.code().unwrap_or(-1), text)
    }

    /// A server on `dir`, on `port`, with the test's paired device.
    fn serve(&self, bin: &Path, dir: &Path, port: u16) -> Kill {
        let devices = json!({"devices": [{"token_id": "e2e-device", "label": "e2e", "auth_version": 1,
            "token_hash": hex(&sha2::Sha256::digest(TOKEN.as_bytes())), "public_key": null,
            "created_at": 1790000000.0, "expires_at": null, "last_used": null}]});
        std::fs::write(dir.join("devices.json"), serde_json::to_string_pretty(&devices).unwrap()).unwrap();
        let child = Command::new(bin)
            .arg("serve")
            .env("AC_CONFIG_DIR", dir)
            .env("BREEZE_HOST", "127.0.0.1")
            .env("BREEZE_PORT", port.to_string())
            .env("BREEZE_LOG", "0")
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        wait_for(&format!("127.0.0.1:{port}"));
        Kill(child)
    }

    fn dir(&self, name: &str, config: Option<Value>) -> PathBuf {
        let d = self.work.join(name);
        std::fs::create_dir_all(&d).unwrap();
        if let Some(c) = config {
            std::fs::write(d.join("config.json"), serde_json::to_string_pretty(&c).unwrap()).unwrap();
        }
        d
    }

    /// The unit's name and whether it is online, as the server says.
    fn state(&self, port: u16, unit: &Value) -> (Value, Value) {
        let (status, v) = call(port, "GET", &format!("/api/units/{}/state", unit["id"]), "");
        assert_eq!(status, 200, "{v}");
        (v["name"].clone(), v["online"].clone())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn tokens_reach_every_kind_of_breeze_core() {
    let vars = ["BREEZE_TOKEN_BIN", "BREEZE_CORE_BIN", "BREEZE_MOCK_BIN", "UCORE_BIN"].map(|v| std::env::var_os(v).map(PathBuf::from));
    let [Some(token_bin), Some(core_bin), Some(mock_bin), Some(ucore_bin)] = vars else {
        eprintln!("skipped: needs BREEZE_TOKEN_BIN (a fake-cloud build), BREEZE_CORE_BIN, BREEZE_MOCK_BIN and UCORE_BIN");
        return;
    };
    if std::env::var_os("BREEZE_DUMMY_SEGMENTS").is_none() {
        eprintln!("skipped: needs BREEZE_DUMMY_SEGMENTS=1 (Linux, CAP_NET_ADMIN)");
        return;
    }
    let work = std::env::temp_dir().join(format!("breeze-token-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&work).unwrap();
    let blueprint = work.join("units.xml");
    std::fs::write(
        &blueprint,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<blueprint xmlns="urn:breeze-mock:blueprint:1" version="1" name="token-e2e" seed="31">
  <environment><constant outdoor="10"/></environment>
  <network><segment name="lan" cidr="10.94.2.0/24" mode="dummy"/></network>
  <unit key="a" segment="lan" ip="10.94.2.2">
    <epithets name="Alpha"/><v3 token="auto" key="auto"/>
    <state power="on" mode="cool" target="24"/><behaviour reply-delay="0ms"/>
  </unit>
  <unit key="b" segment="lan" ip="10.94.2.3">
    <epithets name="Bravo"/><v3 token="auto" key="auto"/>
    <state power="off" mode="heat" target="22"/><behaviour reply-delay="0ms"/>
  </unit>
</blueprint>
"#,
    )
    .unwrap();
    let mock_data = work.join("mock");
    let _mock = Kill(Command::new(&mock_bin).args(["run", blueprint.to_str().unwrap(), "-q"]).env("BREEZE_MOCK_DATA", &mock_data).spawn().unwrap());
    wait_for("10.94.2.3:6444");
    let out = Command::new(&mock_bin)
        .args(["export", "breeze-core-config", blueprint.to_str().unwrap()])
        .env("BREEZE_MOCK_DATA", &mock_data)
        .output()
        .unwrap();
    let exported: Value = serde_json::from_slice(&out.stdout).unwrap();
    let (alpha, bravo) = (exported["units"][0].clone(), exported["units"][1].clone());
    assert_eq!(alpha["ip"], "10.94.2.2");
    let secrets = [&alpha, &bravo].iter().flat_map(|u| [u["token"].as_str().unwrap().to_string(), u["key"].as_str().unwrap().to_string()]).collect();
    // The account: both units, named as the app names them.
    let cloud = work.join("cloud.json");
    let account = |units: &[(&Value, &str)]| {
        json!({"account": ACCOUNT, "password": PASSWORD, "units": units.iter().map(|(u, name)| json!({
            "id": u["id"], "name": name, "token": u["token"], "key": u["key"]})).collect::<Vec<_>>()})
    };
    std::fs::write(&cloud, account(&[(&alpha, "Living room"), (&bravo, "Bedroom")]).to_string()).unwrap();
    let env = Env { token_bin, core_bin, ucore_bin, work: work.clone(), cloud: cloud.clone(), secrets, alpha, bravo };

    // --- 1. A fresh config.json, from a sweep; then both servers drive the units from it.
    let fresh = env.dir("fresh", None);
    let file = fresh.join("config.json");
    let (code, text) = env.token(&["--subnet", "10.94.2.0/24", "--write", file.to_str().unwrap()], PASSWORD, None);
    assert_eq!(code, 0, "{text}");
    let written: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let api_key = written["api_key"].as_str().unwrap().to_string();
    assert_eq!(api_key.len(), 32, "token_urlsafe(24)");
    assert!(!text.contains(&api_key), "the minted API key was printed");
    let units = written["units"].as_array().unwrap();
    assert_eq!(units.len(), 2);
    for (u, name, mock) in [(&units[0], "Living room", &env.alpha), (&units[1], "Bedroom", &env.bravo)] {
        assert_eq!((&u["name"], &u["id"], &u["token"], &u["key"]), (&json!(name), &mock["id"], &mock["token"], &mock["key"]));
    }
    assert!(!fresh.join("config.json.before-breeze-token").exists(), "nothing to back up");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o640);
    }
    // Their own API key, so the test's requests carry it.
    let mut ours = written.clone();
    ours["api_key"] = json!(KEY);
    std::fs::write(&file, serde_json::to_string_pretty(&ours).unwrap()).unwrap();
    for (bin, port) in [(&env.core_bin, 29511u16), (&env.ucore_bin, 29512)] {
        let _server = env.serve(bin, &fresh, port);
        assert_eq!(env.state(port, &env.alpha), (json!("Living room"), json!(true)), "{}", bin.display());
        assert_eq!(env.state(port, &env.bravo), (json!("Bedroom"), json!(true)), "{}", bin.display());
    }

    // --- 2. An existing config.json: the unit there keeps its name, gets its
    // credentials, and the file as it was is kept, byte for byte.
    let before = json!({"api_key": KEY, "units": [{"name": "Kept name", "ip": "10.94.2.99", "port": 6444,
        "id": env.alpha["id"], "token": null, "key": null}]});
    let merge = env.dir("merge", Some(before));
    let file = merge.join("config.json");
    let original = std::fs::read(&file).unwrap();
    let (code, text) = env.token(&["--ip", "10.94.2.2", "--write", file.to_str().unwrap()], PASSWORD, None);
    assert_eq!(code, 0, "{text}");
    assert_eq!(std::fs::read(merge.join("config.json.before-breeze-token")).unwrap(), original);
    let merged: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(merged["api_key"], KEY);
    assert_eq!(merged["units"].as_array().unwrap().len(), 1, "only the unit asked about");
    let u = &merged["units"][0];
    assert_eq!((&u["name"], &u["ip"], &u["token"]), (&json!("Kept name"), &json!("10.94.2.2"), &env.alpha["token"]));
    // Run again: the unit has its token now, and a sweep leaves it alone.
    let (code, text) = env.token(&["--subnet", "10.94.2.0/30", "--write", file.to_str().unwrap()], PASSWORD, None);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("its token and key are there already"), "{text}");

    // --- 3. To a running server: Breeze Core 4.3.2 and μcore, which start
    // with no units. Paired (signed requests), added, unpaired.
    let mut configs = Vec::new();
    for (name, bin, port) in [("to-core", &env.core_bin, 29521u16), ("to-ucore", &env.ucore_bin, 29522)] {
        let dir = env.dir(name, Some(json!({"api_key": KEY, "units": []})));
        let _server = env.serve(bin, &dir, port);
        let (code, text) = env.token(&["--subnet", "10.94.2.0/24", "--to", &format!("127.0.0.1:{port}")], PASSWORD, Some("wrong-key"));
        assert_eq!(code, 1, "{name}: a wrong API key is refused before anything else\n{text}");
        assert!(text.contains("refused that API key") && !text.contains("Signed in"), "{name}: {text}");
        let (code, text) = env.token(&["--subnet", "10.94.2.0/24", "--to", &format!("127.0.0.1:{port}")], PASSWORD, Some(KEY));
        assert_eq!(code, 0, "{name}\n{text}");
        assert!(text.contains("approved from here") && text.contains("is removed again"), "{name}: {text}");
        assert_eq!(env.state(port, &env.alpha), (json!("Living room"), json!(true)), "{name}");
        assert_eq!(env.state(port, &env.bravo), (json!("Bedroom"), json!(true)), "{name}");
        let devices: Value = serde_json::from_slice(&std::fs::read(dir.join("devices.json")).unwrap()).unwrap();
        assert_eq!(devices["devices"].as_array().unwrap().len(), 1, "{name}: its own pairing left behind: {devices}");
        // Again: nothing to do, and still no pairing left behind.
        let (code, text) = env.token(&["--subnet", "10.94.2.0/24", "--to", &format!("127.0.0.1:{port}")], PASSWORD, Some(KEY));
        assert_eq!(code, 0, "{name}\n{text}");
        assert!(text.contains("Nothing here needs a token") && text.contains("is removed again"), "{name}: {text}");
        let devices: Value = serde_json::from_slice(&std::fs::read(dir.join("devices.json")).unwrap()).unwrap();
        assert_eq!(devices["devices"].as_array().unwrap().len(), 1, "{name}: {devices}");
        configs.push(std::fs::read(dir.join("config.json")).unwrap());
    }
    assert_eq!(configs[0], configs[1], "Breeze Core and μcore stored the units differently");

    // --- 4. A wrong password: nothing written, exit 1.
    let wrong = env.dir("wrong", None);
    let (code, text) = env.token(&["--ip", "10.94.2.2", "--write", wrong.join("config.json").to_str().unwrap()], "not the password", None);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("was not accepted"), "{text}");
    assert!(!wrong.join("config.json").exists());

    // --- 5. A unit that is not on the account: the other one still goes.
    std::fs::write(&cloud, account(&[(&env.alpha, "Living room")]).to_string()).unwrap();
    let partial = env.dir("partial", None);
    let (code, text) = env.token(&["--subnet", "10.94.2.0/24", "--write", partial.join("config.json").to_str().unwrap()], PASSWORD, None);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("not on this account"), "{text}");
    let partial: Value = serde_json::from_slice(&std::fs::read(partial.join("config.json")).unwrap()).unwrap();
    assert_eq!(partial["units"].as_array().unwrap().len(), 1);

    // --- 6. A token the unit refuses is not handed over.
    let mut bad = env.alpha.clone();
    bad["key"] = json!("00".repeat(32));
    std::fs::write(&cloud, account(&[(&bad, "Living room")]).to_string()).unwrap();
    let refused = env.dir("refused", None);
    let (code, text) = env.token(&["--ip", "10.94.2.2", "--write", refused.join("config.json").to_str().unwrap()], PASSWORD, None);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("did not accept it"), "{text}");
    assert!(!refused.join("config.json").exists());
    let _ = std::fs::remove_dir_all(&work);
}
