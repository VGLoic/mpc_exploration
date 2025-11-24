use std::{
    env::{self, VarError},
    str::FromStr,
};
use tracing::Level;

pub mod domains;
mod mpc;
pub mod peer_communication;
pub mod routes;

// ############################################
// ################## CONFIG ##################
// ############################################

pub struct Config {
    pub port: u16,
    pub log_level: Level,
    pub server_peer_id: u8,
    pub peers: Vec<Peer>,
}

impl Config {
    pub fn parse_environment() -> Result<Config, anyhow::Error> {
        let mut errors: Vec<String> = vec![];
        let port = match parse_env_variable("PORT") {
            Ok(v) => v.unwrap_or(3000_u16),
            Err(e) => {
                errors.push(e.to_string());
                3000
            }
        };
        // `LOG_LEVEL` has priority over `RUST_LOG`
        let log_level = match parse_env_variable::<Level>("LOG_LEVEL") {
            Ok(v) => v
                .or_else(|| parse_env_variable::<Level>("RUST_LOG").unwrap_or(None))
                .unwrap_or(Level::INFO),
            Err(e) => {
                errors.push(e.to_string());
                Level::INFO
            }
        };

        let server_peer_id = match parse_required_env_variable::<u8>("SERVER_PEER_ID") {
            Ok(v) => v,
            Err(e) => {
                errors.push(e.to_string());
                0
            }
        };

        let peers = match parse_peers() {
            Ok(v) => v,
            Err(e) => {
                errors.push(e.to_string());
                vec![]
            }
        };

        if !errors.is_empty() {
            return Err(anyhow::anyhow!(errors.join(", ")));
        }

        Ok(Config {
            port,
            log_level,
            server_peer_id,
            peers,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: u8,
    pub url: String,
}

impl Peer {
    pub fn new(id: u8, url: String) -> Self {
        Self { id, url }
    }
}

fn parse_peers() -> Result<Vec<Peer>, anyhow::Error> {
    let raw_urls = parse_required_env_variable::<String>("PEER_URLS")?;
    let peer_urls: Vec<String> = raw_urls
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if peer_urls.is_empty() {
        return Err(anyhow::anyhow!("[PEERS]: must contain at least one peer"));
    }
    let peer_url_set = peer_urls
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<String>>();
    if peer_url_set.len() != peer_urls.len() {
        return Err(anyhow::anyhow!("[PEER_URLS]: must contain unique urls"));
    }
    let raw_ids = parse_required_env_variable::<String>("PEER_IDS")?;
    let peer_ids = raw_ids
        .split(',')
        .map(|s| s.trim().parse::<u8>())
        .collect::<Result<Vec<u8>, _>>()?;
    let peer_id_set = peer_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<u8>>();
    if peer_id_set.len() != peer_ids.len() {
        return Err(anyhow::anyhow!("[PEER_IDS]: must contain unique ids"));
    }

    if peer_urls.len() != peer_ids.len() {
        return Err(anyhow::anyhow!(
            "[PEER_URLS] and [PEER_IDS] must have the same number of entries"
        ));
    }

    let peers = peer_urls
        .into_iter()
        .zip(peer_ids)
        .map(|(url, id)| Peer::new(id, url))
        .collect();

    Ok(peers)
}

fn parse_required_env_variable<T>(key: &str) -> Result<T, anyhow::Error>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    match parse_env_variable::<T>(key)? {
        Some(v) => Ok(v),
        None => Err(anyhow::anyhow!("[{key}]: must be specified and non empty")),
    }
}

fn parse_env_variable<T>(key: &str) -> Result<Option<T>, anyhow::Error>
where
    T: FromStr,
    <T as FromStr>::Err: std::error::Error + Send + Sync + 'static,
{
    fn map_err<E>(key: &str, e: E) -> anyhow::Error
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        anyhow::anyhow!("[{key}]: {e}")
    }

    let env_value = match env::var(key) {
        Ok(v) => {
            if v.is_empty() {
                Ok(None)
            } else {
                Ok(Some(v))
            }
        }
        Err(e) => {
            if e == VarError::NotPresent {
                Ok(None)
            } else {
                Err(map_err(key, e))
            }
        }
    }?;
    env_value
        .map(|v| v.parse::<T>().map_err(|e| map_err(key, e)))
        .transpose()
}
