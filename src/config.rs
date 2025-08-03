use eyre::Result;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;
use std::net::Ipv4Addr;

#[derive(Deserialize, Debug)]
pub struct EphemaraConfig {
    pub log_level: String,
    pub xsk_if_name: String,
    pub xsk_mac: String,
    pub xsk_ip: Ipv4Addr,
    pub xsk_port: u16,
    pub gateway_ip: Ipv4Addr,
    pub ws_uri: String,
    pub output_csv_path: String,
}

impl EphemaraConfig {
    pub fn load() -> Result<EphemaraConfig> {
        let config: EphemaraConfig = figment::Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("APP_"))
            .extract()?;
        Ok(config)
    }
}
