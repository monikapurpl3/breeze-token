//! breeze-token -- a V3 air conditioner's token and key, from the Midea
//! account the unit is paired with, for whichever Breeze Core drives it:
//! Breeze μcore, Breeze Core 4, or the Python 3.x line.
//!
//! Breeze Core's own `breeze-core fetch`, standing alone, so that a server
//! that cannot ask Midea itself still gets its units' credentials: μcore has
//! no TLS and no cloud, and the Python line's cloud request stopped working.
//! It runs on any machine on the units' network and keeps `fetch`'s promises:
//!
//! * **The password goes only to a cloud that has the account.** Which clouds
//!   know it is asked first, without it. It is read without echo, used for one
//!   sign-in, and dropped ([`breeze_cloud::Credentials`] scrubs it).
//! * **A token is checked on the unit itself** (a V3 handshake and a state
//!   read, which change nothing) before it goes anywhere.
//! * **Tokens are never printed.** They go into a config.json, backed up first,
//!   or to a server over its own API, and nowhere else.

mod clouds;
mod config_file;
mod lan;
mod server;
mod talk;

use breeze_cloud::{Appliance, Cloud, CloudError, Credentials, Token};
use breeze_proto::discover::{Discovered, Version};

use clouds::{Account, Clouds};
use config_file::ConfigFile;
use server::Server;
use talk::{ask, heading, say, yes_no};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How many times a mistyped password may be typed again.
const PASSWORD_TRIES: usize = 3;

const USAGE: &str = "\
breeze-token -- V3 air conditioners' tokens from Midea's cloud, for Breeze Core

Usage: breeze-token [--ip ADDRESS]... [--subnet CIDR] [--write FILE | --to URL]

Finds the V3 units on this network, signs in to the Midea account they are
paired with, fetches each one's token and key, checks them on the unit
itself, and hands them over -- never printed -- to one of:

  --write FILE      a Breeze Core config.json, made if it is not there (and
                    backed up to FILE.before-breeze-token if it is). Breeze
                    μcore, Breeze Core 4 and Breeze Core 3.x all read it.
  --to URL          a running Breeze μcore or Breeze Core 4, which it pairs
                    with for the moment, adds the units to, and unpairs from.

Without either, it asks.

  --ip ADDRESS      only the unit at this address (again for more); without
                    it, every unit around this machine (its /24)
  --subnet CIDR     every unit in this private subnet (a /22 at most)
  --cloud NAME      smarthome, meiju or nethome: the app the units are in
  --account EMAIL   the account you sign in to that app with
  --password-stdin  read the Midea password as one line of standard input
  -h, --help        this
  -V, --version     the version

BREEZE_API_KEY is the server's API key for --to; without it, it is asked for.
";

/// What the command line asked for. Never a password, or an API key: those
/// are read later, without echo.
#[derive(Debug, Default)]
struct Options {
    ips: Vec<String>,
    subnet: Option<String>,
    write: Option<String>,
    to: Option<String>,
    cloud: Option<String>,
    account: Option<String>,
    password_stdin: bool,
}

enum Parsed {
    Run(Options),
    Help,
    Version,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut o = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |name: &str| inline.clone().or_else(|| args.next()).ok_or_else(|| format!("{name} needs a value"));
        match flag.as_str() {
            "-h" | "--help" => return Ok(Parsed::Help),
            "-V" | "--version" => return Ok(Parsed::Version),
            "--ip" => o.ips.push(value("--ip")?),
            "--subnet" => o.subnet = Some(value("--subnet")?),
            "--write" => o.write = Some(value("--write")?),
            "--to" => o.to = Some(value("--to")?),
            "--cloud" => o.cloud = Some(value("--cloud")?),
            "--account" => o.account = Some(value("--account")?),
            "--password-stdin" => o.password_stdin = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    if o.write.is_some() && o.to.is_some() {
        return Err("--write or --to, not both".into());
    }
    if o.subnet.is_some() && !o.ips.is_empty() {
        return Err("--ip or --subnet, not both".into());
    }
    if let Some(cloud) = &o.cloud {
        parse_cloud(cloud)?;
    }
    if o.write.is_none() && o.to.is_none() && !talk::interactive() {
        return Err("say where the tokens go: --write FILE or --to URL".into());
    }
    Ok(Parsed::Run(o))
}

fn main() {
    let code = match parse(std::env::args().skip(1)) {
        Ok(Parsed::Help) => {
            print!("{USAGE}");
            0
        }
        Ok(Parsed::Version) => {
            println!("breeze-token {VERSION}");
            0
        }
        Ok(Parsed::Run(options)) => match run(options) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("breeze-token: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("breeze-token: {e}\nTry breeze-token --help.");
            2
        }
    };
    std::process::exit(code);
}

/// A unit, its name on the account, and its checked token.
pub struct Fetched {
    pub unit: Discovered,
    pub name: Option<String>,
    pub token: Token,
}

/// Where the tokens go.
enum Destination {
    File(ConfigFile),
    /// The server, and the units it has: id, and whether it holds their
    /// credentials.
    Server(Box<Server>, Vec<(u64, bool)>),
}

impl Destination {
    fn has_credentials(&self, id: u64) -> bool {
        match self {
            Self::File(file) => file.has_credentials(id),
            Self::Server(_, units) => units.iter().any(|(u, has)| *u == id && *has),
        }
    }

    fn deliver(self, got: &[Fetched]) -> Result<usize, String> {
        match self {
            Self::File(file) => file.save(got).map(|()| 0),
            Self::Server(server, units) => {
                heading(&format!("Adding them to {}", server.base()));
                let mut failed = 0;
                for f in got {
                    let ip = f.unit.ip.to_string();
                    let known = units.iter().any(|(u, _)| *u == f.unit.id);
                    let name = if known { None } else { Some(f.name.clone().unwrap_or_else(|| format!("AC {ip}"))) };
                    let shown = name.clone().unwrap_or_else(|| format!("the unit at {ip}"));
                    match server.add(&ip, name.as_deref(), &f.token) {
                        Ok(()) if known => println!("  OK   {shown}: it had this unit already, and now its token and key."),
                        Ok(()) => println!("  OK   {shown}: added."),
                        Err(e) => {
                            failed += 1;
                            println!("  !!   {shown}: the server refused it: {e}");
                        }
                    }
                }
                if failed > 0 {
                    say(
                        "\nThe server probes a unit's address itself before adding it, so it has to reach \
                         the unit as this machine does.",
                    );
                }
                Ok(failed)
                // `server` drops here, and removes its pairing.
            }
        }
    }
}

fn run(options: Options) -> Result<i32, String> {
    let clouds = clouds::clouds()?;
    heading("What this does");
    say(
        "A V3 air conditioner will not talk to anything without a token and a key, and only Midea \
         hands them out -- these days only to the account the unit is paired with in Midea's app. \
         So this finds the units, signs in to that account once, asks for their tokens, and checks \
         each one on the unit itself before handing it over.",
    );
    say(
        "Your password is used for that one sign-in and then forgotten. Neither it nor the tokens \
         are ever logged or shown.",
    );

    // --- where they go: first, so anything wrong there is found before anyone signs in
    let destination = destination(&options)?;

    // --- which units -------------------------------------------------------
    heading("Finding the units");
    let look = match (&options.subnet, options.ips.is_empty()) {
        (Some(cidr), _) => lan::Look::Subnet(cidr),
        (None, false) => lan::Look::At(&options.ips),
        (None, true) => lan::Look::Around,
    };
    let found = lan::find(look)?;
    if found.is_empty() {
        say(
            "No units answered. Make sure they are powered on and on the same network as this \
             machine, or name one you know with --ip (your router's DHCP leases say where they are).",
        );
        return Ok(1);
    }
    let named = !options.ips.is_empty();
    let mut wanted = Vec::new();
    for unit in found {
        let what = match unit.version {
            _ if unit.device_type != 0xAC => format!("not an air conditioner (type 0x{:02X}), skipping it", unit.device_type),
            Version::V3 if destination.has_credentials(unit.id) && !named => {
                "V3, and its token and key are there already (name it with --ip to fetch again)".to_string()
            }
            Version::V3 => "V3, needs a token and key".to_string(),
            other => format!("{other:?}: needs no token, skipping it"),
        };
        println!("  {:<16} id {:<16} {what}", unit.ip, unit.id);
        if unit.device_type == 0xAC && unit.version == Version::V3 && (named || !destination.has_credentials(unit.id)) {
            wanted.push(unit);
        }
    }
    if wanted.is_empty() {
        say("\nNothing here needs a token.");
        return Ok(0);
    }

    // --- which app, which account --------------------------------------------
    let choice = match &options.cloud {
        Some(name) => parse_cloud(name)?,
        None => ask_app()?,
    };
    let account = match &options.account {
        Some(a) => a.trim().to_string(),
        None => loop {
            let typed = ask("\nThe email (or phone number) you sign in to that app with: ")?.ok_or("no account given")?;
            if !typed.is_empty() {
                break typed;
            }
        },
    };

    heading("Which of Midea's clouds knows this account");
    say("Asking each of them by name only. No password is sent for this.");
    let known = who_knows(clouds.as_ref(), &account, choice);
    let cloud = match decide(choice, &known)? {
        Some(cloud) => cloud,
        None => {
            // Not echoed: this output is what people paste into a bug report.
            say(
                "None of them has that account. Check how it is spelled -- it has to be exactly what \
                 you sign in to the app with -- or use the account of whoever set the units up.",
            );
            return Ok(1);
        }
    };
    if cloud == Cloud::NetHomePlus {
        return nethome(clouds.as_ref(), &account, &options, &known, &wanted);
    }
    let Some(session) = sign_in(clouds.as_ref(), cloud, &account, &options)? else {
        return Ok(1);
    };

    let (got, missing) = fetch_all(cloud, session.as_ref(), &wanted)?;
    drop(session);
    if got.is_empty() {
        say("\nNothing was handed over; nothing was changed.");
        return Ok(1);
    }
    let refused = destination.deliver(&got)?;
    Ok(if missing + refused == 0 { 0 } else { 1 })
}

// --- the destination -----------------------------------------------------------

fn destination(options: &Options) -> Result<Destination, String> {
    let (file, url) = match (&options.write, &options.to) {
        (Some(path), _) => (Some(path.clone()), None),
        (_, Some(url)) => (None, Some(url.clone())),
        _ => {
            heading("Where the tokens go");
            println!("  1  a config.json    for a server to read when it starts (any Breeze Core),");
            println!("                      or to keep as a copy");
            println!("  2  a running server Breeze μcore or Breeze Core 4, by its address");
            loop {
                match ask("\nChoose 1 or 2: ")?.as_deref() {
                    Some("1") => {
                        let path = ask("The file [config.json]: ")?.unwrap_or_default();
                        break (Some(if path.is_empty() { "config.json".into() } else { path }), None);
                    }
                    Some("2") => match ask("The server's address (e.g. 192.168.1.10): ")? {
                        Some(url) if !url.is_empty() => break (None, Some(url)),
                        Some(_) => {}
                        None => return Err("no answer".into()),
                    },
                    Some(_) => println!("  1 or 2, please."),
                    None => return Err("no answer".into()),
                }
            }
        }
    };
    if let Some(path) = file {
        let file = ConfigFile::open(&path)?;
        if options.write.is_some() {
            heading("Where the tokens go");
        }
        println!("  Into {}.", file.path());
        return Ok(Destination::File(file));
    }
    let url = url.unwrap_or_default();
    heading("The server");
    let mut server = Server::connect(&url)?;
    server.pair()?;
    let units = server.units()?;
    Ok(Destination::Server(Box::new(server), units))
}

// --- choosing ------------------------------------------------------------------

/// What the person said about the app, before anything is asked of a cloud.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    App(Cloud),
    NotSure,
}

fn parse_cloud(name: &str) -> Result<Choice, String> {
    match name.to_ascii_lowercase().replace(['-', ' ', '_'], "").as_str() {
        "smarthome" | "msmarthome" => Ok(Choice::App(Cloud::SmartHome)),
        "meiju" => Ok(Choice::App(Cloud::Meiju)),
        "nethome" | "nethomeplus" => Ok(Choice::App(Cloud::NetHomePlus)),
        _ => Err(format!("unknown --cloud '{name}': smarthome, meiju or nethome")),
    }
}

fn ask_app() -> Result<Choice, String> {
    heading("Which app the units are paired with");
    say("The app you set the air conditioners up in, on your phone:");
    println!("  1  MSmartHome     Midea's current app, outside China");
    println!("  2  NetHome Plus   the older app many units came with");
    println!("  3  Meiju          美的美居, in mainland China");
    println!("  4  not sure       ask each of them about your account");
    loop {
        match ask("\nChoose 1-4: ")?.as_deref().map(str::trim) {
            Some("1") => return Ok(Choice::App(Cloud::SmartHome)),
            Some("2") => return Ok(Choice::App(Cloud::NetHomePlus)),
            Some("3") => return Ok(Choice::App(Cloud::Meiju)),
            Some("4") => return Ok(Choice::NotSure),
            Some(_) => println!("  1, 2, 3 or 4, please."),
            None => return Err("no answer".into()),
        }
    }
}

/// Whether each cloud has the account, printed as it goes. A cloud that
/// cannot be reached is `None`, and said so, rather than taken as a no.
fn who_knows(clouds: &dyn Clouds, account: &str, choice: Choice) -> Vec<(Cloud, Option<bool>)> {
    // The chosen one first; then the others, because the most useful answer
    // to "MSmartHome doesn't know me" is which app does.
    let mut order = vec![Cloud::SmartHome, Cloud::NetHomePlus, Cloud::Meiju];
    if let Choice::App(c) = choice {
        order.retain(|o| *o != c);
        order.insert(0, c);
    }
    order
        .into_iter()
        .map(|cloud| {
            let answer = match clouds.account_known(cloud, account) {
                Ok(known) => {
                    println!("  {:<16} {}", cloud.short_name(), if known { "has this account" } else { "does not know it" });
                    Some(known)
                }
                Err(e) => {
                    println!("  {:<16} could not be asked: {e}", cloud.short_name());
                    None
                }
            };
            (cloud, answer)
        })
        .collect()
}

fn knows(known: &[(Cloud, Option<bool>)], cloud: Cloud) -> bool {
    known.iter().any(|(c, k)| *c == cloud && *k == Some(true))
}

/// Which cloud to sign in to, given what was chosen and who knows the
/// account. `None` when nobody does.
fn decide(choice: Choice, known: &[(Cloud, Option<bool>)]) -> Result<Option<Cloud>, String> {
    // Where a token can actually come from, in the order worth trying.
    let issuing = [Cloud::SmartHome, Cloud::Meiju].into_iter().find(|c| knows(known, *c));
    match choice {
        Choice::App(Cloud::NetHomePlus) => {
            if let Some(other) = issuing {
                say(&format!(
                    "\n{} has this account too, and unlike NetHome Plus it still hands out tokens -- if \
                     the units are paired there.",
                    other.app_name()
                ));
                if yes_no(&format!("Use {} instead?", other.app_name()), true)? {
                    return Ok(Some(other));
                }
            }
            Ok(knows(known, Cloud::NetHomePlus).then_some(Cloud::NetHomePlus))
        }
        Choice::App(cloud) if knows(known, cloud) => Ok(Some(cloud)),
        Choice::App(cloud) => {
            say(&format!(
                "\n{} does not know this account. It said so before any password was sent, so this is \
                 not about the password.",
                cloud.app_name()
            ));
            if let Some(other) = issuing {
                say(&format!("{} does, so I'll use that.", other.app_name()));
                return Ok(Some(other));
            }
            if knows(known, Cloud::NetHomePlus) {
                say(
                    "NetHome Plus does: this is a NetHome Plus account. Accounts made there do not exist \
                     in MSmartHome -- Midea keeps them on separate systems -- so here is what that means \
                     for the token.",
                );
                return Ok(Some(Cloud::NetHomePlus));
            }
            Ok(None)
        }
        Choice::NotSure => Ok(issuing.or_else(|| knows(known, Cloud::NetHomePlus).then_some(Cloud::NetHomePlus))),
    }
}

// --- signing in ------------------------------------------------------------------

fn password(account: &str, cloud: Cloud, options: &Options) -> Result<String, String> {
    if options.password_stdin {
        return talk::stdin_line("the password");
    }
    talk::secret(&format!("\nPassword for {account} on {} (it will not show as you type): ", cloud.app_name()))
}

/// Sign in, letting a mistyped password be typed again.
fn sign_in(clouds: &dyn Clouds, cloud: Cloud, account: &str, options: &Options) -> Result<Option<Box<dyn Account>>, String> {
    let tries = if options.password_stdin { 1 } else { PASSWORD_TRIES };
    for attempt in 1..=tries {
        let credentials = Credentials::new(account, password(account, cloud, options)?, "");
        match clouds.login(cloud, &credentials) {
            Ok(session) => {
                println!("  Signed in to {}.", cloud.app_name());
                return Ok(Some(session));
            }
            Err(CloudError::Api { code: 3101 | 3102, .. }) if attempt < tries => {
                let left = tries - attempt;
                println!("  That password was not accepted. ({left} more {})", if left == 1 { "try" } else { "tries" });
            }
            Err(e) => {
                println!("  Could not sign in: {e}");
                say(e.advice());
                return Ok(None);
            }
        }
    }
    Ok(None)
}

// --- fetching ----------------------------------------------------------------------

/// The units' tokens, each checked on its unit, and how many units got none.
fn fetch_all(cloud: Cloud, session: &dyn Account, wanted: &[Discovered]) -> Result<(Vec<Fetched>, usize), String> {
    // The account's own list: the units' names, and a way to explain a unit
    // that is not on it rather than report a bare refusal.
    let listed: Option<Vec<Appliance>> = match session.appliances() {
        Ok(Some(list)) => {
            heading(&format!("Units on this {} account", cloud.app_name()));
            if list.is_empty() {
                say("None at all. The units have to be added in the app first.");
            }
            for a in &list {
                let here = wanted.iter().find(|u| u.id == a.id).map(|u| format!("  (at {})", u.ip)).unwrap_or_default();
                println!("  {:<20} {}{here}", a.name, if a.online { "online" } else { "offline" });
            }
            Some(list)
        }
        Ok(None) => None,
        Err(e) => {
            println!("  (could not list the account's units: {e})");
            None
        }
    };

    heading("Fetching and checking");
    let mut got = Vec::new();
    let mut missing = 0;
    for unit in wanted {
        let name = listed.as_ref().and_then(|l| l.iter().find(|a| a.id == unit.id)).map(|a| a.name.clone());
        let shown = name.clone().unwrap_or_else(|| format!("the unit at {}", unit.ip));
        if listed.is_some() && name.is_none() {
            missing += 1;
            println!("  {shown}: not on this account.");
            say(&format!(
                "    {} only gives a unit's token to the account it is paired with, and this unit is not \
                 one of the units listed above. If someone else set it up, fetch with their account; if \
                 it is still in another app, it has to be added in {} first.",
                cloud.app_name(),
                cloud.app_name()
            ));
            continue;
        }
        match session.token(unit.id) {
            Ok(token) => {
                println!("  {shown}: token received. Trying it on the unit itself...");
                match lan::works(unit, &token) {
                    Ok(()) => {
                        println!("  OK   {shown} accepted it: a V3 handshake and a state read, nothing changed on the unit.");
                        got.push(Fetched { unit: unit.clone(), name, token });
                    }
                    Err(e) => {
                        println!("  !!   {shown} did not accept it: {e}");
                        say(
                            "    That usually means the unit is off or cannot be reached from here; it can \
                             also mean the unit was paired again since, and has a newer token.",
                        );
                        if talk::interactive() && yes_no("    Hand it over anyway? It is the token Midea holds for this unit.", false)? {
                            got.push(Fetched { unit: unit.clone(), name, token });
                        } else {
                            missing += 1;
                        }
                    }
                }
            }
            Err(e) => {
                missing += 1;
                println!("  {shown}: no token -- {e}");
                say(&format!("    {}", e.advice()));
            }
        }
    }
    Ok((got, missing))
}

// --- NetHome Plus ------------------------------------------------------------------

fn nethome(clouds: &dyn Clouds, account: &str, options: &Options, known: &[(Cloud, Option<bool>)], wanted: &[Discovered]) -> Result<i32, String> {
    heading("NetHome Plus");
    say(
        "NetHome Plus still lets you sign in, but since August 2026 it has refused to hand out tokens \
         -- for every unit, its owners' included -- and nobody has found a way round that. I'll try \
         once, in case it has changed.",
    );
    if let Some(session) = sign_in(clouds, Cloud::NetHomePlus, account, options)? {
        match session.token(wanted[0].id) {
            Ok(_) => say("It answered with a token -- NetHome Plus is issuing them again. Run this once more and choose NetHome Plus."),
            Err(e) => println!("  As expected, it refused: {e}"),
        }
    }
    heading("What works instead");
    say(
        "MSmartHome, Midea's current app, still hands out tokens -- to the account the unit is paired \
         with. A NetHome Plus login is not an MSmartHome account (Midea keeps them on separate \
         systems), so the unit has to move:",
    );
    println!("  1. Install MSmartHome on your phone.");
    if knows(known, Cloud::SmartHome) {
        println!("  2. Sign in with the same email or phone number -- MSmartHome already");
        println!("     knows it.");
    } else {
        println!("  2. Create an account in it. The same email is fine; it is a new account.");
    }
    println!("  3. Add the air conditioner in MSmartHome. The app walks you through putting");
    println!("     the unit into pairing mode and onto your Wi-Fi again.");
    println!("  4. Run breeze-token again and choose MSmartHome.");
    say(
        "\nOnly do this for units that need a token now. Pairing a unit again gives it a new token, so \
         one a server already holds for it will most likely stop working.",
    );
    say(
        "\nIf you have a unit's token and key from somewhere else -- an old Home Assistant setup, or \
         msmart-ng's output -- a server takes them directly: POST /api/units, or `breeze-core pair`.",
    );
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Result<Parsed, String> {
        parse(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn options_are_read_with_or_without_an_equals_sign() {
        let Ok(Parsed::Run(o)) = args(&["--ip", "192.168.1.50", "--ip=192.168.1.51", "--to=core.lan", "--cloud", "MSmartHome"]) else {
            panic!("not parsed");
        };
        assert_eq!(o.ips, ["192.168.1.50", "192.168.1.51"]);
        assert_eq!(o.to.as_deref(), Some("core.lan"));
        assert!(matches!(args(&["--help"]), Ok(Parsed::Help)));
        assert!(matches!(args(&["-V"]), Ok(Parsed::Version)));
    }

    #[test]
    fn contradictions_and_typos_are_refused() {
        for bad in [
            &["--write", "a.json", "--to", "core.lan"][..],
            &["--ip", "192.168.1.5", "--subnet", "192.168.1.0/24"],
            &["--cloud", "alexa"],
            &["--ip"],
            &["--password", "hunter2"],
        ] {
            assert!(args(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_cloud_flag_takes_the_names_people_use() {
        for name in ["smarthome", "MSmartHome", "msmart-home"] {
            assert_eq!(parse_cloud(name), Ok(Choice::App(Cloud::SmartHome)));
        }
        assert_eq!(parse_cloud("NetHome Plus"), Ok(Choice::App(Cloud::NetHomePlus)));
        assert_eq!(parse_cloud("meiju"), Ok(Choice::App(Cloud::Meiju)));
    }

    #[test]
    fn a_nethome_account_known_to_smarthome_too_is_sent_to_smarthome_when_unsure() {
        let known = [(Cloud::SmartHome, Some(true)), (Cloud::NetHomePlus, Some(true)), (Cloud::Meiju, Some(false))];
        assert_eq!(decide(Choice::NotSure, &known).unwrap(), Some(Cloud::SmartHome));
    }

    #[test]
    fn an_account_only_nethome_knows_goes_to_the_nethome_help() {
        let known = [(Cloud::SmartHome, Some(false)), (Cloud::NetHomePlus, Some(true)), (Cloud::Meiju, Some(false))];
        assert_eq!(decide(Choice::NotSure, &known).unwrap(), Some(Cloud::NetHomePlus));
        assert_eq!(decide(Choice::App(Cloud::SmartHome), &known).unwrap(), Some(Cloud::NetHomePlus));
    }

    #[test]
    fn nobody_knowing_the_account_is_none_and_unreachable_is_not_a_no() {
        let known = [(Cloud::SmartHome, Some(false)), (Cloud::NetHomePlus, None), (Cloud::Meiju, Some(false))];
        assert_eq!(decide(Choice::NotSure, &known).unwrap(), None);
        assert!(!knows(&known, Cloud::NetHomePlus));
    }
}
