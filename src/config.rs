use eyre::Result;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Deserialize, Debug)]
pub struct EphemaraConfig {
    pub log_level: String,
    pub xdp_if_name: String,
    pub xdp_mac: String,
    pub xdp_ip: IpAddr,
    // TODO: We don't want xdp handle all the IP packet. Since a xdp listener need to know the
    // remote ip first to extend the rule, but extend the rule also need to know the ip first, now
    // we only support `client connect` operation to extend rules automatically.
    pub xdp_acceptable_ips: Vec<IpAddr>,
    pub xdp_skb_mod: bool,
    pub gateway_ip: Ipv4Addr,
    pub ws_uri: String,
    pub output_csv_path: String,
}

impl EphemaraConfig {
    pub fn load() -> Result<EphemaraConfig> {
        let config: EphemaraConfig = figment::Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("EPHEMARA_"))
            .extract()?;
        Ok(config)
    }
}
