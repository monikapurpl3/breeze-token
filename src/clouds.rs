//! Midea's clouds, through breeze-cloud. A trait only so that the end-to-end
//! test can stand a pretend cloud in their place (the `fake-cloud` feature):
//! breeze-cloud's own requests are tested in Breeze Core, against its mock of
//! the real API.

use breeze_cloud::{Appliance, Cloud, CloudError, Credentials, Token};

pub trait Clouds {
    /// Whether `cloud` has the account, asked without the password.
    fn account_known(&self, cloud: Cloud, account: &str) -> Result<bool, CloudError>;
    fn login(&self, cloud: Cloud, credentials: &Credentials) -> Result<Box<dyn Account>, CloudError>;
}

/// A signed-in account.
pub trait Account {
    /// The units paired with it, where the cloud will say.
    fn appliances(&self) -> Result<Option<Vec<Appliance>>, CloudError>;
    fn token(&self, device_id: u64) -> Result<Token, CloudError>;
}

/// The real ones.
pub struct Midea;

impl Clouds for Midea {
    fn account_known(&self, cloud: Cloud, account: &str) -> Result<bool, CloudError> {
        cloud.account_known(account)
    }

    fn login(&self, cloud: Cloud, credentials: &Credentials) -> Result<Box<dyn Account>, CloudError> {
        Ok(Box::new(cloud.login(credentials)?))
    }
}

impl Account for breeze_cloud::Session {
    fn appliances(&self) -> Result<Option<Vec<Appliance>>, CloudError> {
        breeze_cloud::Session::appliances(self)
    }

    fn token(&self, device_id: u64) -> Result<Token, CloudError> {
        breeze_cloud::Session::token(self, device_id)
    }
}

/// The clouds to use: the real ones, or in a `fake-cloud` build, the pretend
/// one its environment names.
pub fn clouds() -> Result<Box<dyn Clouds>, String> {
    #[cfg(feature = "fake-cloud")]
    if let Some(path) = std::env::var_os("BREEZE_TOKEN_FAKE_CLOUD") {
        eprintln!("(a test build: the Midea cloud is pretend, from {})", path.to_string_lossy());
        return Ok(Box::new(fake::Fake::load(std::path::Path::new(&path))?));
    }
    Ok(Box::new(Midea))
}

#[cfg(feature = "fake-cloud")]
mod fake {
    //! An MSmartHome account read from a file:
    //! `{"account", "password", "units": [{"id", "name", "token", "key"}]}`.
    //! Only MSmartHome knows it; a wrong password is 3102, as the real one
    //! answers; a unit not listed has no token.

    use super::*;
    use serde_json::Value;

    pub struct Fake(Value);

    impl Fake {
        pub fn load(path: &std::path::Path) -> Result<Self, String> {
            let text = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            serde_json::from_slice(&text).map(Fake).map_err(|e| format!("{}: {e}", path.display()))
        }

        fn units(&self) -> impl Iterator<Item = &Value> {
            self.0["units"].as_array().into_iter().flatten()
        }
    }

    impl Clouds for Fake {
        fn account_known(&self, cloud: Cloud, account: &str) -> Result<bool, CloudError> {
            Ok(cloud == Cloud::SmartHome && self.0["account"].as_str() == Some(account))
        }

        fn login(&self, cloud: Cloud, credentials: &Credentials) -> Result<Box<dyn Account>, CloudError> {
            if !self.account_known(cloud, &credentials.account)? || self.0["password"].as_str() != Some(credentials.password()) {
                return Err(CloudError::Api { code: 3102, message: "account or password error".into() });
            }
            Ok(Box::new(Fake(self.0.clone())))
        }
    }

    impl Account for Fake {
        fn appliances(&self) -> Result<Option<Vec<Appliance>>, CloudError> {
            Ok(Some(
                self.units()
                    .map(|u| Appliance {
                        id: u["id"].as_u64().unwrap_or(0),
                        name: u["name"].as_str().unwrap_or("unnamed").into(),
                        kind: Some(0xAC),
                        online: true,
                    })
                    .collect(),
            ))
        }

        fn token(&self, device_id: u64) -> Result<Token, CloudError> {
            self.units()
                .find(|u| u["id"].as_u64() == Some(device_id))
                .and_then(|u| Some(Token { token: u["token"].as_str()?.into(), key: u["key"].as_str()?.into() }))
                .ok_or_else(|| CloudError::NoToken("the cloud listed 0 token(s), none of them for this unit".into()))
        }
    }
}
