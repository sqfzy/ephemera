use ephemera::af_xdp::device::XdpDevice;
use ephemera::af_xdp::listener::XdpTcpListener;
use ephemera::af_xdp::reactor::XdpReactor;
use ephemera::af_xdp::stream::{XdpTcpStream, XdpTcpStreamBorrow};
use smoltcp::socket::tcp::{Socket, SocketBuffer};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;

// Test constants
const INTERFACE_NAME1: &str = "test_iface1";
const INTERFACE_MAC1: &str = "2a:2b:72:fb:e8:cc";
const INTERFACE_IP1: &str = "192.168.10.1";

const INTERFACE_NAME2: &str = "test_iface2";
const INTERFACE_MAC2: &str = "ea:d8:f6:0e:76:01";
const INTERFACE_IP2: &str = "192.168.10.2";

struct SetupGuard {}

impl Drop for SetupGuard {
    fn drop(&mut self) {
        Command::new("sudo")
            .arg("nu")
            .arg("tests/cleanup_net.nu")
            .status()
            .expect("Failed to execute cleanup_net.nu script");
    }
}

static SETUP: LazyLock<SetupGuard> = LazyLock::new(|| {
    let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::from_str(&level).unwrap())
        .init();

    Command::new("sudo")
        .arg("nu")
        .arg("tests/setup_net.nu")
        .output()
        .expect("Failed to execute setup_net.nu script");

    SetupGuard {}
});

#[test]
fn test_xdp_device_connect_and_send() {
    let _ = &*SETUP;

    // --- Reactor 1 (Server) Setup ---
    let mac_addr1 = INTERFACE_MAC1.parse::<EthernetAddress>().unwrap();
    let ip_addr1 = IpAddress::from_str(INTERFACE_IP1).unwrap();
    let device1: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME1).unwrap();
    let mut reactor1 = XdpReactor::new(device1, mac_addr1);
    reactor1.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr1, 24)).unwrap();
    });
    let server_socket = Socket::new(
        SocketBuffer::new(vec![0; 4096]),
        SocketBuffer::new(vec![0; 4096]),
    );
    reactor1.sockets.add(server_socket);

    let server_port = 443;
    let server_endpoint = IpEndpoint::new(ip_addr1, server_port);

    let mut server = XdpTcpListener::bind_with_reactor(server_endpoint, &mut reactor1).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = server.accept_with_reactor(&mut reactor1).unwrap();

        // let mut buf = String::new();
        // stream.read_to_string(&mut buf).unwrap();
        let mut buf = [0; 1024];
        let n = stream.read(&mut buf).unwrap();

        assert_eq!(&buf[..n], b"Hello XDP Server!");

        stream.write_all(b"Hello XDP Client!").unwrap();
        stream.flush().unwrap();

        stream.shutdown().unwrap();
    });

    // --- Reactor 2 (Client) Setup ---
    let mac_addr2 = INTERFACE_MAC2.parse::<EthernetAddress>().unwrap();
    let ip_addr2 = IpAddress::from_str(INTERFACE_IP2).unwrap();
    let device2: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME2).unwrap();
    let mut reactor2 = XdpReactor::new(device2, mac_addr2);
    reactor2.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr2, 24)).unwrap();
    });
    let client_socket = Socket::new(
        SocketBuffer::new(vec![0; 4096]),
        SocketBuffer::new(vec![0; 4096]),
    );
    reactor2.sockets.add(client_socket);

    let mut client = XdpTcpStreamBorrow::connect(
        SocketAddr::from((server_endpoint.addr, server_endpoint.port)),
        &mut reactor2,
    )
    .unwrap();

    client.write_all(b"Hello XDP Server!").unwrap();
    client.flush().unwrap();

    let mut buf = String::new();
    client.read_to_string(&mut buf).unwrap();

    assert_eq!(&buf, "Hello XDP Client!");
}
