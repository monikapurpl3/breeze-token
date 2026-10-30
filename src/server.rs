//! `--to`: a running server, over its own API. It pairs as a device would
//! (approving itself from the server's network, as breezectl does), adds the
//! units with `POST /api/units`, and removes its pairing again when done.

use std::cell::Cell;
use std::time::Duration;

use base64::Engine as _;
use breeze_cloud::Token;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha3::Digest;

use crate::talk::{self, say};

/// How many times a mistyped API key may be typed again.
const KEY_TRIES: usize = 3;

enum Auth {
    /// A server from before device pairing: the API key is everything.
    KeyOnly,
    Bearer(String),
    /// Auth version 2: every request signed with this machine's own key.
    Signed { key_id: String, signing: Box<SigningKey> },
}

pub struct Server {
    base: String,
    key: String,
    agent: ureq::Agent,
    features: Vec<String>,
    auth: Auth,
    /// Our pairing, to be removed when done.
    token_id: Option<String>,
    label: String,
    /// Seconds to add to this machine's clock, if the server said it is off.
    skew: Cell<i64>,
}

/// A server's address as people type it: `192.168.1.10`, `host:8420`, a URL.
fn normalise(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (s.to_ascii_lowercase(), r),
        None => ("http".to_string(), url),
    };
    let host = rest.split('/').next().unwrap_or(rest);
    // A bracketed IPv6 address has colons of its own.
    let has_port = host.rsplit_once(':').is_some_and(|(h, p)| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (!h.contains(':') || h.ends_with(']')));
    if scheme == "http" && !has_port {
        format!("{scheme}://{rest}:8420")
    } else {
        format!("{scheme}://{rest}")
    }
}

/// What a refusal says, for a person.
fn detail(status: u16, v: &Value) -> String {
    match &v["detail"] {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get("message").or_else(|| o.get("error")).and_then(Value::as_str).unwrap_or("refused").to_string(),
        _ => format!("HTTP {status}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut out = [0u8; N];
    getrandom::getrandom(&mut out).map_err(|e| format!("no randomness: {e}"))?;
    Ok(out)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Server {
    /// Find the server and check its API key. Nothing is changed on it yet.
    pub fn connect(url: &str) -> Result<Self, String> {
        let base = normalise(url);
        let mut server = Self {
            agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build(),
            base,
            key: String::new(),
            features: Vec::new(),
            auth: Auth::KeyOnly,
            token_id: None,
            label: format!("breeze-token on {}", talk::this_host()),
            skew: Cell::new(0),
        };
        match server.call("GET", "/api/health", None)? {
            (200, v) if v["status"] == "ok" => {}
            _ => return Err(format!("{} answered, but not as a Breeze Core server does", server.base)),
        }
        let from_env = std::env::var("BREEZE_API_KEY").ok().filter(|k| !k.trim().is_empty());
        let tries = if from_env.is_some() { 1 } else { KEY_TRIES };
        for attempt in 1..=tries {
            server.key = match &from_env {
                Some(k) => k.trim().to_string(),
                None => {
                    if attempt == 1 {
                        say("Its API key is in its config.json (\"api_key\"), or ask whoever runs it.");
                    }
                    talk::secret("  API key (it will not show as you type): ")?.trim().to_string()
                }
            };
            match server.call("GET", "/api/version", None)? {
                (200, v) => {
                    let version = v["version"].as_str().unwrap_or("?").to_string();
                    let name = v["name"].as_str().unwrap_or("Breeze Core").to_string();
                    server.features = v["features"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                    if version.starts_with("3.") || version.starts_with("2.") {
                        return Err(format!(
                            "{} runs Breeze Core {version}, which asks Midea's cloud for a token itself whenever a \
                             unit is added -- and that request no longer works, so it cannot add a unit at all. \
                             Write its config.json instead (--write), and restart it.",
                            server.base
                        ));
                    }
                    if !server.features.iter().any(|f| f == "config_api") {
                        return Err(format!("{name} {version} at {} cannot add units over its API. Write its config.json instead (--write).", server.base));
                    }
                    println!("  The key is right. {} runs {name} {version}.", server.base);
                    return Ok(server);
                }
                (401, _) if attempt < tries => println!("  The server refused that API key."),
                (401, _) => return Err("the server refused that API key".into()),
                (status, v) => return Err(format!("{}: {}", server.base, detail(status, &v))),
            }
        }
        Err("no API key".into())
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Pair as a device: a code, approved from here if this machine is on
    /// the server's network, or by someone who is.
    pub fn pair(&mut self) -> Result<(), String> {
        if !self.features.iter().any(|f| f == "device_pairing") {
            return Ok(());
        }
        let signed = self.features.iter().any(|f| f == "ed25519_auth");
        let signing = SigningKey::from_bytes(&random::<32>()?);
        let start = if signed {
            let public = base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes());
            json!({"label": self.label, "auth_version": 2, "public_key": public})
        } else {
            json!({"label": self.label, "auth_version": 1})
        };
        let (status, s) = self.call("POST", "/api/auth/enroll/start", Some(&start))?;
        if status != 200 {
            return Err(format!("the server would not start a pairing: {}", detail(status, &s)));
        }
        let (Some(session), Some(code)) = (s["session_id"].as_str(), s["user_code"].as_str()) else {
            return Err("the server started a pairing but sent no code".into());
        };
        let ttl = s["expires_in"].as_f64().unwrap_or(60.0) as u64;
        match self.call("POST", "/api/auth/enroll/approve", Some(&json!({"code": code})))? {
            (200, _) => say(&format!("Paired as \"{}\", approved from here: this machine is on the server's network.", self.label)),
            _ => {
                say(&format!("Pairing code: {code}"));
                say(&format!("It has to be approved on the server's own network within {ttl} seconds: in its web panel, or on it with `breeze-core approve {code}`. Waiting... (Ctrl-C to give up)"));
            }
        }
        let poll = json!({"session_id": session});
        let deadline = std::time::Instant::now() + Duration::from_secs(ttl + 5);
        let answer = loop {
            let (status, p) = self.call("POST", "/api/auth/enroll/poll", Some(&poll))?;
            if status != 200 {
                return Err(format!("the pairing failed: {}", detail(status, &p)));
            }
            if p["status"] != "pending" {
                break p;
            }
            if std::time::Instant::now() > deadline {
                return Err("the pairing was not approved in time".into());
            }
            std::thread::sleep(Duration::from_secs(2));
        };
        if answer["status"] != "approved" {
            return Err(format!("the pairing was not approved ({})", answer["status"].as_str().unwrap_or("?")));
        }
        let token_id = answer["token_id"].as_str().ok_or("the server approved the pairing but named no device")?.to_string();
        self.auth = if signed {
            Auth::Signed { key_id: token_id.clone(), signing: Box::new(signing) }
        } else {
            Auth::Bearer(answer["device_token"].as_str().ok_or("the server approved the pairing but issued no token")?.to_string())
        };
        self.token_id = Some(token_id);
        Ok(())
    }

    /// The units the server has: id, and whether it holds their credentials.
    pub fn units(&self) -> Result<Vec<(u64, bool)>, String> {
        match self.call("GET", "/api/config", None)? {
            (200, v) => Ok(v["units"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|u| {
                    let id = u["id"].as_str().and_then(|s| s.parse().ok()).or_else(|| u["id"].as_u64())?;
                    Some((id, u["has_v3_credentials"].as_bool().unwrap_or(false)))
                })
                .collect()),
            (status, v) => Err(format!("cannot read the server's units: {}", detail(status, &v))),
        }
    }

    /// `POST /api/units`: the server probes the address itself, and adds the
    /// unit, or updates it if it is there already. A name only for a new one:
    /// left out, the server keeps the name it has.
    pub fn add(&self, ip: &str, name: Option<&str>, token: &Token) -> Result<(), String> {
        let mut body = json!({"ip": ip, "token": token.token, "key": token.key});
        if let Some(name) = name {
            body["name"] = json!(name);
        }
        match self.call("POST", "/api/units", Some(&body))? {
            (200 | 201, _) => Ok(()),
            (status, v) => Err(detail(status, &v)),
        }
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
        let first = self.once(method, path, body)?;
        // A signature can be turned away for this machine's clock, or by
        // chance; the server says which, and once more is then right.
        if let (401, Auth::Signed { .. }) = (first.0, &self.auth) {
            let reason = first.1["detail"]["error"].as_str().unwrap_or("");
            if first.1["detail"]["retryable"] == true {
                if reason == "clock_skew" {
                    if let Some(server_time) = first.1["detail"]["server_time"].as_f64() {
                        self.skew.set(server_time as i64 - unix_now());
                    }
                }
                return self.once(method, path, body);
            }
        }
        Ok(first)
    }

    fn once(&self, method: &str, path: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
        let text = body.map(Value::to_string).unwrap_or_default();
        let mut request = self.agent.request(method, &format!("{}{path}", self.base));
        if !self.key.is_empty() {
            request = request.set("X-API-Key", &self.key);
        }
        match &self.auth {
            Auth::KeyOnly => {}
            Auth::Bearer(token) => request = request.set("Authorization", &format!("Bearer {token}")),
            Auth::Signed { key_id, signing } => {
                let ts = (unix_now() + self.skew.get()).to_string();
                let nonce = hex(&random::<16>()?);
                let canonical = format!(
                    "breeze-auth-v2\n{}\n{path}\n{ts}\n{nonce}\n{}",
                    method.to_ascii_uppercase(),
                    hex(&sha3::Sha3_512::digest(text.as_bytes()))
                );
                let signature = base64::engine::general_purpose::STANDARD.encode(signing.sign(canonical.as_bytes()).to_bytes());
                request = request
                    .set("X-Breeze-Auth-Version", "2")
                    .set("X-Breeze-Key-Id", key_id)
                    .set("X-Breeze-Timestamp", &ts)
                    .set("X-Breeze-Nonce", &nonce)
                    .set("X-Breeze-Signature", &signature);
            }
        }
        let result = match body {
            Some(_) => request.set("Content-Type", "application/json").send_string(&text),
            None => request.call(),
        };
        let response = match result {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(format!("cannot reach {}: {e}", self.base)),
        };
        let status = response.status();
        let text = response.into_string().unwrap_or_default();
        Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
    }
}

impl Drop for Server {
    /// Leave no pairing behind: it was for this one run.
    fn drop(&mut self) {
        let Some(token_id) = self.token_id.take() else { return };
        match self.once("DELETE", &format!("/api/auth/devices/{token_id}"), None) {
            Ok((200 | 204, _)) => println!("  Its pairing with {} is removed again.", self.base),
            _ => say(&format!(
                "Its pairing with {} (\"{}\") could not be removed. Remove it in the server's list of \
                 devices: it is not needed any more.",
                self.base, self.label
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_as_people_type_them() {
        assert_eq!(normalise("192.168.1.10"), "http://192.168.1.10:8420");
        assert_eq!(normalise("192.168.1.10:9000/"), "http://192.168.1.10:9000");
        assert_eq!(normalise("HTTP://core.lan"), "http://core.lan:8420");
        assert_eq!(normalise("https://breeze.example.com"), "https://breeze.example.com");
        assert_eq!(normalise("http://[fd00::5]"), "http://[fd00::5]:8420");
        assert_eq!(normalise("http://[fd00::5]:81"), "http://[fd00::5]:81");
    }

    #[test]
    fn a_refusal_reads_either_shape() {
        assert_eq!(detail(422, &json!({"detail": "ip is required"})), "ip is required");
        assert_eq!(detail(401, &json!({"detail": {"error": "replay", "retryable": true}})), "replay");
        assert_eq!(detail(502, &Value::Null), "HTTP 502");
    }
}
