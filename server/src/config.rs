use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// An admin token shorter than this is refused: it is the only thing
/// protecting the moderation endpoints.
const MIN_ADMIN_TOKEN_LENGTH: usize = 16;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub db_path: PathBuf,
    pub admin_token: Option<String>,
    pub firmware_dir: Option<PathBuf>,
    /// Take the client address from the last `X-Forwarded-For` entry. Only
    /// correct behind a reverse proxy that appends the real peer address.
    pub trust_forwarded_for: bool,
    pub write_limit: u32,
    pub write_window: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 8787)),
            db_path: PathBuf::from("./jmk-registry.db"),
            admin_token: None,
            firmware_dir: None,
            trust_forwarded_for: false,
            write_limit: 30,
            write_window: Duration::from_secs(600),
        }
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mut config = Self::default();
        if let Some(bind) = variable("JMK_BIND") {
            config.bind = bind
                .parse()
                .map_err(|_| format!("JMK_BIND is not an address like 127.0.0.1:8787: {bind}"))?;
        }
        if let Some(path) = variable("JMK_DB") {
            config.db_path = PathBuf::from(path);
        }
        if let Some(token) = variable("JMK_ADMIN_TOKEN") {
            if token.len() < MIN_ADMIN_TOKEN_LENGTH {
                return Err(format!(
                    "JMK_ADMIN_TOKEN must be at least {MIN_ADMIN_TOKEN_LENGTH} characters"
                ));
            }
            config.admin_token = Some(token);
        }
        config.firmware_dir = variable("JMK_FIRMWARE_DIR").map(PathBuf::from);
        if let Some(value) = variable("JMK_TRUST_FORWARDED_FOR") {
            config.trust_forwarded_for = match value.as_str() {
                "1" | "true" => true,
                "0" | "false" => false,
                _ => return Err(format!("JMK_TRUST_FORWARDED_FOR must be true or false: {value}")),
            };
        }
        Ok(config)
    }
}

/// An unset and an empty variable mean the same thing, so that
/// `JMK_ADMIN_TOKEN=` in an env file cannot enable an empty admin token.
fn variable(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}
