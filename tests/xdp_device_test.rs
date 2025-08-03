use ephemera::af_xdp::device::XdpDevice;
use ephemera::af_xdp::reactor::XdpReactor;
use smoltcp::socket::tcp::{Socket, SocketBuffer, State};
use smoltcp::time::Duration;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint};
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;
use portpicker::pick_unused_port;

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
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    let status = Command::new("sudo")
        .arg("nu")
        .arg("tests/setup_net.nu")
        .status()
        .expect("Failed to execute setup_net.nu script");
    assert!(status.success(), "setup_net.nu script failed");

    SetupGuard {}
});

fn poll_reactors(reactor1: &mut XdpReactor, reactor2: &mut XdpReactor) {
    for _ in 0..100 {
        reactor1.poll(Some(Duration::from_millis(10))).unwrap();
        reactor2.poll(Some(Duration::from_millis(10))).unwrap();
    }
}

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
    let handle1 = reactor1.sockets.add(server_socket);

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
    let handle2 = reactor2.sockets.add(client_socket);

    // --- Connection Logic ---
    let server_port = 443;
    let server_endpoint = IpEndpoint::new(ip_addr1, server_port);

    // Server starts listening
    reactor1
        .sockets
        .get_mut::<Socket>(handle1)
        .listen(server_endpoint)
        .unwrap();
    assert_eq!(reactor1.sockets.get_mut::<Socket>(handle1).state(), State::Listen);

    // Client initiates connection
    let local_port = pick_unused_port().unwrap();
    reactor2
        .sockets
        .get_mut::<Socket>(handle2)
        .connect(reactor2.iface.context(), server_endpoint, local_port)
        .unwrap();
    assert_eq!(reactor2.sockets.get_mut::<Socket>(handle2).state(), State::SynSent);

    // Poll until connection is established
    println!("Polling for connection...");
    for _ in 0..100 {
        reactor1.poll(Some(Duration::from_millis(10))).unwrap();
        reactor2.poll(Some(Duration::from_millis(10))).unwrap();
        if reactor1.sockets.get::<Socket>(handle1).state() == State::Established
            && reactor2.sockets.get::<Socket>(handle2).state() == State::Established
        {
            break;
        }
    }
    println!("Polling complete.");

    assert_eq!(reactor1.sockets.get::<Socket>(handle1).state(), State::Established, "Server should be established");
    assert_eq!(reactor2.sockets.get::<Socket>(handle2).state(), State::Established, "Client should be established");

    // --- Data Transfer ---
    let client_msg = b"hello from client";
    let server_msg = b"hello from server";

    // Client sends data
    let bytes_sent = reactor2.sockets.get_mut::<Socket>(handle2).send_slice(client_msg).unwrap();
    assert_eq!(bytes_sent, client_msg.len());
    poll_reactors(&mut reactor1, &mut reactor2);

    // Server receives data
    let mut recv_buf = [0u8; 1024];
    let bytes_recvd = reactor1.sockets.get_mut::<Socket>(handle1).recv_slice(&mut recv_buf).unwrap();
    assert_eq!(bytes_recvd, client_msg.len());
    assert_eq!(&recv_buf[..bytes_recvd], client_msg);

    // Server sends data
    let bytes_sent = reactor1.sockets.get_mut::<Socket>(handle1).send_slice(server_msg).unwrap();
    assert_eq!(bytes_sent, server_msg.len());
    poll_reactors(&mut reactor1, &mut reactor2);

    // Client receives data
    let bytes_recvd = reactor2.sockets.get_mut::<Socket>(handle2).recv_slice(&mut recv_buf).unwrap();
    assert_eq!(bytes_recvd, server_msg.len());
    assert_eq!(&recv_buf[..bytes_recvd], server_msg);
}
