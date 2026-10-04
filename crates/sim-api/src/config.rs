//! Server configuration, parsed once at startup so a bad value fails before
//! the server binds.

use std::fmt;

use axum::http::HeaderValue;

/// The port the frontend's Vite dev proxy targets.
pub const DEFAULT_PORT: u16 = 3000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    /// The static frontend's origin, allowed by CORS. `None` means same-origin only.
    pub allowed_origin: Option<HeaderValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    InvalidPort(String),
    InvalidOrigin(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidPort(raw) => write!(f, "PORT {raw:?} isn't a port number"),
            ConfigError::InvalidOrigin(raw) => write!(
                f,
                "ALLOWED_ORIGIN {raw:?} must be one exact origin, such as https://example.app"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Takes the raw `PORT` and `ALLOWED_ORIGIN` values. Either may be unset.
    pub fn parse(port: Option<&str>, allowed_origin: Option<&str>) -> Result<Config, ConfigError> {
        let port = match port {
            None => DEFAULT_PORT,
            Some(raw) => raw
                .parse()
                .map_err(|_| ConfigError::InvalidPort(raw.to_string()))?,
        };
        let allowed_origin = allowed_origin.map(parse_origin).transpose()?;
        Ok(Config {
            port,
            allowed_origin,
        })
    }
}

/// One exact origin. A wildcard would let any site call the API, and an empty
/// value is almost certainly a misconfigured secret.
fn parse_origin(raw: &str) -> Result<HeaderValue, ConfigError> {
    let invalid = || ConfigError::InvalidOrigin(raw.to_string());
    if raw.is_empty() || raw == "*" {
        return Err(invalid());
    }
    HeaderValue::from_str(raw).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_values_get_defaults() {
        assert_eq!(
            Config::parse(None, None),
            Ok(Config {
                port: DEFAULT_PORT,
                allowed_origin: None
            })
        );
    }

    #[test]
    fn valid_values_are_used() {
        let config = Config::parse(Some("8080"), Some("https://rails.example")).unwrap();
        assert_eq!(config.port, 8080);
        assert_eq!(
            config.allowed_origin,
            Some(HeaderValue::from_static("https://rails.example"))
        );
    }

    #[test]
    fn a_bad_port_is_rejected() {
        for raw in ["", "abc", "70000", "-1"] {
            assert_eq!(
                Config::parse(Some(raw), None),
                Err(ConfigError::InvalidPort(raw.to_string()))
            );
        }
    }

    #[test]
    fn a_bad_origin_is_rejected() {
        for raw in ["", "*", "bad\nvalue"] {
            assert_eq!(
                Config::parse(None, Some(raw)),
                Err(ConfigError::InvalidOrigin(raw.to_string()))
            );
        }
    }
}
