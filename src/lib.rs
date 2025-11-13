use std::{
    env::{self, VarError},
    net::SocketAddr,
    str::FromStr,
};
use tracing::Level;

pub mod routes;

// ############################################
// ################## CONFIG ##################
// ############################################

pub struct Config {
    pub port: u16,
    pub log_level: Level,
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
            peers,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub addr: SocketAddr,
    pub id: u8,
}

impl Peer {
    pub fn new(addr: SocketAddr, id: u8) -> Self {
        Self { addr, id }
    }
}

fn parse_peers() -> Result<Vec<Peer>, anyhow::Error> {
    let raw_urls = parse_required_env_variable::<String>("PEER_ADDRESSES")?;
    let peer_addresses: Vec<SocketAddr> = raw_urls
        .split(',')
        .map(|s| s.trim().parse())
        .collect::<Result<Vec<SocketAddr>, _>>()?;
    if peer_addresses.is_empty() {
        return Err(anyhow::anyhow!("[PEERS]: must contain at least one peer"));
    }
    let peer_url_set = peer_addresses
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<SocketAddr>>();
    if peer_url_set.len() != peer_addresses.len() {
        return Err(anyhow::anyhow!(
            "[PEER_ADDRESSES]: must contain unique urls"
        ));
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

    if peer_addresses.len() != peer_ids.len() {
        return Err(anyhow::anyhow!(
            "[PEER_ADDRESSES] and [PEER_IDS] must have the same number of entries"
        ));
    }

    let peers = peer_addresses
        .into_iter()
        .zip(peer_ids)
        .map(|(url, id)| Peer::new(url, id))
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
