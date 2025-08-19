use std::process::Command;
use std::str::FromStr;
use std::sync::OnceLock;

pub const INTERFACE_NAME1: &str = "test_iface1";
pub const INTERFACE_MAC1: &str = "2a:2b:72:fb:e8:cc";
pub const INTERFACE_IP1: &str = "192.168.2.9";

pub const INTERFACE_NAME2: &str = "test_iface2";
pub const INTERFACE_MAC2: &str = "ea:d8:f6:0e:76:01";
pub const INTERFACE_IP2: &str = "192.168.2.10";

/// 需要先运行setup_net.nu
pub fn setup() {
    static START: OnceLock<()> = OnceLock::new();

    START.get_or_init(|| {
        let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::from_str(&level).unwrap())
            .init();
    });
}
