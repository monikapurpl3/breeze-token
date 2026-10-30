//! The units on this network: which answer, which need a token, and whether a
//! token works on one. All of it Breeze Core's own discovery and handshake.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::time::Duration;

use breeze_cloud::Token;
use breeze_device::{Device, ScanReport, UnitConfig};
use breeze_proto::discover::Discovered;

/// Long enough for a unit on Wi-Fi to answer; Breeze Core's own window.
const LISTEN: Duration = Duration::from_secs(3);

/// Where to look.
pub enum Look<'a> {
    /// The /24 around this machine, swept and broadcast to.
    Around,
    /// A private subnet, swept.
    Subnet(&'a str),
    /// These addresses only, each asked directly.
    At(&'a [String]),
}

/// Every unit that answered, once each, by address.
pub fn find(look: Look) -> Result<Vec<Discovered>, String> {
    let reports: Vec<ScanReport> = match look {
        Look::Around => vec![breeze_device::scan(LISTEN).map_err(|e| format!("discovery failed: {e}"))?],
        Look::Subnet(cidr) => vec![breeze_device::scan_subnet(cidr, LISTEN).map_err(|e| format!("discovery failed: {e}"))?],
        // A /32 is a sweep of one host: the reply can only be from that
        // address. All at once, so ten addresses take three seconds, not 30.
        Look::At(addresses) => {
            let asks: Vec<_> = addresses
                .iter()
                .map(|ip| {
                    let cidr = format!("{}/32", ip.trim());
                    std::thread::spawn(move || breeze_device::scan_subnet(&cidr, LISTEN))
                })
                .collect();
            let mut out = Vec::new();
            for (ask, ip) in asks.into_iter().zip(addresses) {
                match ask.join() {
                    Ok(Ok(report)) => out.push(report),
                    Ok(Err(e)) => return Err(format!("{ip}: {e}")),
                    Err(_) => return Err(format!("{ip}: discovery failed")),
                }
            }
            out
        }
    };
    let mut found = BTreeMap::new();
    for report in reports {
        for (ip, why) in &report.unparseable {
            println!("  something at {ip} answered but could not be read: {why}");
        }
        for unit in report.found {
            found.entry(unit.id).or_insert(unit);
        }
    }
    let mut found: Vec<Discovered> = found.into_values().collect();
    found.sort_by_key(|u| u.ip);
    Ok(found)
}

/// Try a token on the unit: a V3 handshake and a state read, which change
/// nothing on it.
pub fn works(unit: &Discovered, token: &Token) -> Result<(), String> {
    let (Some(bytes), Some(key)) = (unhex(&token.token), unhex(&token.key).and_then(|k| <[u8; 32]>::try_from(k).ok())) else {
        return Err("the token Midea sent is not the right shape".into());
    };
    let config = UnitConfig {
        id: unit.id,
        name: String::new(),
        ip: IpAddr::V4(unit.ip),
        port: unit.port,
        token: Some(bytes),
        key: Some(key),
    };
    Device::new(config).refresh().map(|_| ()).map_err(|e| e.to_string())
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_read_strictly() {
        assert_eq!(unhex("00ff10"), Some(vec![0, 255, 16]));
        assert_eq!(unhex("0"), None);
        assert_eq!(unhex("zz"), None);
        assert_eq!(unhex("é0"), None);
    }
}
