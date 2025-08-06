use ephemera::af_xdp::device::XdpDevice;
use ephemera::af_xdp::listener::XdpTcpListener;
use ephemera::af_xdp::reactor::XdpReactor;
use ephemera::af_xdp::stream::{XdpTcpStream, XdpTcpStreamBorrow};
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::process::Command;
use std::str::FromStr;
use std::sync::OnceLock;

const INTERFACE_NAME1: &str = "test_iface1";
const INTERFACE_MAC1: &str = "2a:2b:72:fb:e8:cc";
const INTERFACE_IP1: &str = "192.168.10.1";

const INTERFACE_NAME2: &str = "test_iface2";
const INTERFACE_MAC2: &str = "ea:d8:f6:0e:76:01";
const INTERFACE_IP2: &str = "192.168.10.2";

fn setup() {
    static START: OnceLock<()> = OnceLock::new();

    START.get_or_init(|| {
        unsafe {
            std::env::set_var("EPHEMARA_XDP_IF_NAME", INTERFACE_NAME2);
            std::env::set_var("EPHEMARA_XDP_MAC", INTERFACE_MAC2);
            std::env::set_var("EPHEMARA_XDP_IP", INTERFACE_IP2);
        }

        let level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::from_str(&level).unwrap())
            .init();

        let output = Command::new("sudo")
            .arg("nu")
            .arg("tests/setup_net.nu")
            .output()
            .expect("Failed to execute setup_net.nu script");

        assert!(output.status.success(), "Setup script failed");
    });
}

fn server_reactor() -> XdpReactor<1024> {
    let mac_addr = INTERFACE_MAC1.parse::<EthernetAddress>().unwrap();
    let ip_addr = IpAddress::from_str(INTERFACE_IP1).unwrap();
    let device: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME1).unwrap();
    let mut reactor = XdpReactor::new(device, mac_addr);
    reactor.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr, 24)).unwrap();
    });
    reactor
}

fn client_reactor() -> XdpReactor<1024> {
    let mac_addr = INTERFACE_MAC2.parse::<EthernetAddress>().unwrap();
    let ip_addr = IpAddress::from_str(INTERFACE_IP2).unwrap();
    let device: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME2).unwrap();
    let mut reactor = XdpReactor::new(device, mac_addr);
    reactor.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr, 24)).unwrap();
    });
    reactor
}

#[test]
fn test_xdp_connect() {
    setup();

    let mut server_reactor = server_reactor();

    let server_port = 443;
    let server_endpoint = IpEndpoint::new(IpAddress::from_str(INTERFACE_IP1).unwrap(), server_port);

    let mut server =
        XdpTcpListener::bind_with_reactor(server_endpoint, &mut server_reactor).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = server.accept_with_reactor(&mut server_reactor).unwrap();

        stream.shutdown().unwrap();
    });

    let mut client_reactor = client_reactor();

    let mut client = XdpTcpStreamBorrow::connect(
        SocketAddr::from((server_endpoint.addr, server_endpoint.port)),
        &mut client_reactor,
    )
    .unwrap();

    client.shutdown().unwrap();
}

#[test]
fn test_xdp_multiple_connect() {
    setup();

    let ip = std::env::var("EPHEMARA_XDP_IP").unwrap();
    let addr = (ip, 443);

    let mut server = XdpTcpListener::bind(addr.clone()).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();

        std::thread::spawn(move || {
            stream.read_to_end(&mut vec![]).unwrap();
        });
    });

    let handles = (0..10)
        .map(|i| {
            let addr = addr.clone();
            std::thread::spawn(move || {
                let mut client = XdpTcpStream::connect(addr.clone()).unwrap();
                println!("Client {i} debug0");
                client.shutdown().unwrap();
                println!("Client {i} finished");
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

#[test]
fn test_xdp_send_and_recv() {
    setup();

    let mut server_reactor = server_reactor();

    let server_port = 443;
    let server_endpoint = IpEndpoint::new(
        server_reactor.iface.ipv4_addr().unwrap().into(),
        server_port,
    );

    let mut server =
        XdpTcpListener::bind_with_reactor(server_endpoint, &mut server_reactor).unwrap();

    std::thread::spawn(move || {
        let (mut stream, _) = server.accept_with_reactor(&mut server_reactor).unwrap();

        let mut buf = [0; 1024];
        let n = stream.read(&mut buf).unwrap();

        assert_eq!(&buf[..n], b"Hello XDP Server!");

        stream.write_all(b"Hello XDP Client!").unwrap();
        stream.flush().unwrap();

        stream.shutdown().unwrap();
    });

    let mut client_reactor = client_reactor();

    let mut client = XdpTcpStreamBorrow::connect(
        SocketAddr::from((server_endpoint.addr, server_endpoint.port)),
        &mut client_reactor,
    )
    .unwrap();

    client.write_all(b"Hello XDP Server!").unwrap();
    client.flush().unwrap();

    let mut buf = String::new();
    client.read_to_string(&mut buf).unwrap();

    assert_eq!(&buf, "Hello XDP Client!");

    let mut buf = [0; 1024];
    let _ = client.read(&mut buf).unwrap();

    client.shutdown().unwrap();
}

#[test]
fn test_xdp_multiple_send_and_recv() {
    setup();

    let mut server_reactor = server_reactor();

    let server_port = 443;
    let server_endpoint = IpEndpoint::new(
        server_reactor.iface.ipv4_addr().unwrap().into(),
        server_port,
    );

    let mut server =
        XdpTcpListener::bind_with_reactor(server_endpoint, &mut server_reactor).unwrap();

    std::thread::spawn(move || {
        loop {
            let (mut stream, _) = server.accept_with_reactor(&mut server_reactor).unwrap();

            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).unwrap();

            stream.write_all(&buf[..n]).unwrap();
            stream.flush().unwrap();

            stream.shutdown().unwrap();
        }
    });

    let addr = SocketAddr::from((server_endpoint.addr, server_endpoint.port));

    let handles = (0..10)
        .map(|i| {
            std::thread::spawn(move || {
                let mut client = XdpTcpStream::connect(addr).unwrap();
                client
                    .write_all(format!("Hello XDP {i}!").as_bytes())
                    .unwrap();
                client.flush().unwrap();

                let mut buf = String::new();
                client.read_to_string(&mut buf).unwrap();
                assert_eq!(buf, format!("Hello XDP {i}!"));

                client.shutdown().unwrap();

                println!("Client {i} finished");
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}
