use std::env;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub cors_origins: Vec<String>,
    pub session_ttl: Duration,
    /// Atrás de proxy reverso confiável, o IP real vem de X-Forwarded-For.
    pub trust_proxy: bool,
    pub log_json: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("configuração inválida: {0}")]
pub struct ConfigError(String);

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError("DATABASE_URL é obrigatório".into()))?;
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .map_err(|_| ConfigError("BIND_ADDR deve ser host:porta".into()))?;
        let session_days: u64 = env::var("SESSION_TTL_DAYS")
            .unwrap_or_else(|_| "14".into())
            .parse()
            .ok()
            .filter(|d| (1..=90).contains(d))
            .ok_or_else(|| ConfigError("SESSION_TTL_DAYS deve estar entre 1 e 90".into()))?;
        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5173".into())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Self {
            database_url,
            bind_addr,
            cors_origins,
            session_ttl: Duration::from_secs(session_days * 24 * 60 * 60),
            trust_proxy: env::var("TRUST_PROXY").is_ok_and(|v| v == "true"),
            log_json: env::var("LOG_FORMAT").is_ok_and(|v| v == "json"),
        })
    }
}
