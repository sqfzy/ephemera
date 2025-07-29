use eyre::Result;
use figment::providers::{Format, Toml};
use serde::Deserialize;
use std::net::Ipv4Addr;

#[derive(Deserialize, Debug)]
pub struct AppConfig {
    pub log_level: String,
    pub xsk_if_name: String,
    pub xsk_mac: String,
    pub xsk_ip: Ipv4Addr,
    pub xsk_port: u16,
    pub gateway_ip: Ipv4Addr,
    pub ws_uri: String,
    pub output_csv_path: String,
}

impl AppConfig {
    pub fn load() -> Result<AppConfig> {
        let config: AppConfig = figment::Figment::new()
            .merge(Toml::file("config.toml"))
            .extract()?;
        Ok(config)
    }
}
