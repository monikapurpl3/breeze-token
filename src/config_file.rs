//! `--write`: Breeze Core's config.json, which Breeze μcore, Breeze Core 4 and
//! the Python 3.x line all read, written by Breeze Core's own store code.

use base64::Engine as _;
use breeze_store::models::{AppConfig, UnitConfig};
use breeze_store::{Mode, StoreError};

use crate::talk::say;
use crate::Fetched;

pub struct ConfigFile {
    path: String,
    config: AppConfig,
    existed: bool,
}

impl ConfigFile {
    /// Read it now, so a file that is not a config.json is said before anyone
    /// signs in anywhere.
    pub fn open(path: &str) -> Result<Self, String> {
        match breeze_store::load::<AppConfig>(path) {
            Ok(config) => Ok(Self { path: path.into(), existed: std::path::Path::new(path).exists(), config }),
            Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self { path: path.into(), config: AppConfig::default(), existed: false })
            }
            // Somebody's file: reported, never replaced.
            Err(e) => Err(format!("cannot use {path}: {e}")),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn has_credentials(&self, id: u64) -> bool {
        self.unit(id).is_some_and(|u| u.token.is_some() && u.key.is_some())
    }

    fn unit(&self, id: u64) -> Option<&UnitConfig> {
        self.config.find_unit(&id.to_string())
    }

    /// Add the units, or give the ones already there their new address and
    /// credentials (their names are kept). The file as it was goes to
    /// `FILE.before-breeze-token` first.
    pub fn save(mut self, got: &[Fetched]) -> Result<(), String> {
        let backup = format!("{}.before-breeze-token", self.path);
        if self.existed {
            // The bytes, mode included, not a re-serialisation of them.
            std::fs::copy(&self.path, &backup).map_err(|e| format!("cannot write the backup {backup}: {e}"))?;
        }
        let minted = self.config.api_key.is_none();
        if minted {
            // The shape Breeze Core mints (`secrets.token_urlsafe(24)`).
            let mut bytes = [0u8; 24];
            getrandom::getrandom(&mut bytes).map_err(|e| format!("cannot mint an API key: {e}"))?;
            self.config.api_key = Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes));
        }
        for f in got {
            let id = i64::try_from(f.unit.id).map_err(|_| format!("unit id {} is out of range", f.unit.id))?;
            let ip = f.unit.ip.to_string();
            match self.config.find_unit_mut(&id.to_string()) {
                Some(unit) => {
                    unit.ip = ip;
                    unit.port = f.unit.port;
                    unit.token = Some(f.token.token.clone());
                    unit.key = Some(f.token.key.clone());
                }
                None => self.config.units.push(UnitConfig {
                    name: f.name.clone().unwrap_or_else(|| format!("AC {ip}")),
                    ip,
                    port: f.unit.port,
                    id,
                    token: Some(f.token.token.clone()),
                    key: Some(f.token.key.clone()),
                }),
            }
        }
        breeze_store::save(&self.path, &self.config, Mode::AdminReadable).map_err(|e| format!("cannot write {}: {e}", self.path))?;

        crate::talk::heading("Saved");
        let names: Vec<&str> = got.iter().filter_map(|f| self.unit(f.unit.id)).map(|u| u.name.as_str()).collect();
        say(&format!("{} now in {}.", names.join(", "), self.path));
        if self.existed {
            say(&format!("The version before this is in {backup}."));
        }
        if minted {
            say(
                "It had no API key, so one was made. It is in the file as \"api_key\": the app and \
                 breezectl pair with it.",
            );
        }
        say(
            "\nA server reads config.json when it starts. Put this one where it looks (AC_CONFIG_DIR; \
             /etc/breeze-core for a package install) and restart it.",
        );
        say(
            "\nKeep a copy of it somewhere else too. A unit never forgets its token, but Midea may stop \
             handing them out altogether, and then that copy is the only one there is.",
        );
        Ok(())
    }
}
