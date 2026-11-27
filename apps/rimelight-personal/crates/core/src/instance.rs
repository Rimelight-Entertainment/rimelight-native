//! Where one Rimelight Personal instance lives: the API it talks to and the Cloudflare Access application in front of it.
//!
//! There is no default. Core does not know any deployment; each app builds an [`InstanceConfig`] at start-up with
//! [`InstanceConfig::from_env`] and passes it on. Access scopes its application to the API root, so `api_base` is also the
//! Access application URL.

/// One instance's addresses and Access identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceConfig {
    /// The API root, for example `https://example.com/api/personal` (no trailing slash).
    pub api_base: String,
    /// The Access application's AUD tag. Access prints it as `kid` in the login redirect; it is not a secret.
    pub access_aud: String,
    /// The Access team's login host, for example `https://team.cloudflareaccess.com`.
    pub access_team_origin: String,
}

impl InstanceConfig {
    /// Reads the instance from the environment, and names the first variable that is missing.
    ///
    /// Native builds read `RIMELIGHT_API_BASE`, `RIMELIGHT_ACCESS_AUD` and `RIMELIGHT_ACCESS_TEAM_ORIGIN` at run time.
    /// The web build has no environment at run time, so it reads the same names as build-time values (`option_env!`).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_env() -> Result<Self, String> {
        let read = |name: &str| {
            std::env::var(name)
                .map_err(|_| format!("set {name} to the instance this app connects to"))
                .map(|value| value.trim().to_owned())
        };
        Self::from_values(
            read("RIMELIGHT_API_BASE")?,
            read("RIMELIGHT_ACCESS_AUD")?,
            read("RIMELIGHT_ACCESS_TEAM_ORIGIN")?,
        )
    }

    /// The web build's instance, from the values set when it was built.
    #[cfg(target_arch = "wasm32")]
    pub fn from_env() -> Result<Self, String> {
        Self::from_values(
            option_env!("RIMELIGHT_API_BASE")
                .unwrap_or_default()
                .to_owned(),
            option_env!("RIMELIGHT_ACCESS_AUD")
                .unwrap_or_default()
                .to_owned(),
            option_env!("RIMELIGHT_ACCESS_TEAM_ORIGIN")
                .unwrap_or_default()
                .to_owned(),
        )
    }

    /// Checks and trims the three values. Each must be set, and the API root must be an absolute URL.
    pub fn from_values(
        api_base: String,
        access_aud: String,
        access_team_origin: String,
    ) -> Result<Self, String> {
        let api_base = api_base.trim().trim_end_matches('/').to_owned();
        if !(api_base.starts_with("https://") || api_base.starts_with("http://")) {
            return Err(format!(
                "RIMELIGHT_API_BASE must be an http(s) URL, got {api_base:?}"
            ));
        }
        let access_aud = access_aud.trim().to_owned();
        if access_aud.is_empty() {
            return Err("RIMELIGHT_ACCESS_AUD is empty".to_owned());
        }
        let access_team_origin = access_team_origin.trim().trim_end_matches('/').to_owned();
        if access_team_origin.is_empty() {
            return Err("RIMELIGHT_ACCESS_TEAM_ORIGIN is empty".to_owned());
        }
        Ok(Self {
            api_base,
            access_aud,
            access_team_origin,
        })
    }

    /// The origin (scheme and host) of the API, for example `https://example.com`.
    pub fn app_origin(&self) -> &str {
        match self.api_base.find("://") {
            Some(scheme_end) => {
                let after = scheme_end + 3;
                match self.api_base[after..].find('/') {
                    Some(path) => &self.api_base[..after + path],
                    None => &self.api_base,
                }
            }
            None => &self.api_base,
        }
    }

    /// The host part of [`InstanceConfig::app_origin`], which Access uses as the application's name in login URLs.
    pub fn app_host(&self) -> &str {
        let origin = self.app_origin();
        origin.split_once("://").map_or(origin, |(_, host)| host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(api_base: &str) -> InstanceConfig {
        InstanceConfig {
            api_base: api_base.to_owned(),
            access_aud: "AUD".to_owned(),
            access_team_origin: "https://team.example".to_owned(),
        }
    }

    #[test]
    fn origin_and_host_come_from_the_api_root() {
        let c = config("https://example.com/api/personal");
        assert_eq!(c.app_origin(), "https://example.com");
        assert_eq!(c.app_host(), "example.com");
        assert_eq!(
            config("http://localhost:5173").app_origin(),
            "http://localhost:5173"
        );
        assert_eq!(config("http://localhost:5173").app_host(), "localhost:5173");
    }

    #[test]
    fn values_are_trimmed_and_checked() {
        let ok = InstanceConfig::from_values(
            " https://example.com/api/ ".to_owned(),
            " AUD ".to_owned(),
            "https://team.example/".to_owned(),
        )
        .expect("valid");
        assert_eq!(ok.api_base, "https://example.com/api");
        assert_eq!(ok.access_aud, "AUD");
        assert_eq!(ok.access_team_origin, "https://team.example");
        assert!(
            InstanceConfig::from_values("example.com".into(), "A".into(), "https://t".into())
                .is_err()
        );
        assert!(
            InstanceConfig::from_values("https://e".into(), "".into(), "https://t".into()).is_err()
        );
        assert!(InstanceConfig::from_values("https://e".into(), "A".into(), " ".into()).is_err());
    }
}
