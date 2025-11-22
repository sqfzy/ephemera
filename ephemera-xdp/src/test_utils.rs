use crate::reactor::XdpReactor;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp::{Socket, SocketBuffer},
};
use std::{str::FromStr, sync::OnceLock};

pub(crate) const INTERFACE_NAME1: &str = "test_iface1";
pub(crate) const INTERFACE_IP1: &str = "192.168.2.9";

pub(crate) const INTERFACE_NAME2: &str = "test_iface2";
pub(crate) const INTERFACE_IP2: &str = "192.168.2.10";

pub fn setup() {
    static START: OnceLock<()> = OnceLock::new();

    START.get_or_init(|| {
        let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::from_str(&level).unwrap())
            .init();
    });
}

pub(crate) fn create_reactor1() -> XdpReactor {
    XdpReactor::builder()
        .if_name(INTERFACE_NAME1)
        .build()
        .unwrap()
}

pub(crate) fn create_reactor2() -> XdpReactor {
    XdpReactor::builder()
        .if_name(INTERFACE_NAME2)
        .build()
        .unwrap()
}

pub(crate) fn add_tcp_socket(reactor: &XdpReactor) -> SocketHandle {
    let mut reactor = reactor.lock().unwrap();
    reactor.sockets.add(Socket::new(
        SocketBuffer::new(vec![0; 4096]),
        SocketBuffer::new(vec![0; 4096]),
    ))
}
