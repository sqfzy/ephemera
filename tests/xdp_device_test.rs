use af_xdp::af_xdp::device::XdpDevice;
use af_xdp::af_xdp::reactor::XdpReactor;
use af_xdp::af_xdp::stream::XdpStream;
use smoltcp::iface::{Interface, SocketSet};
use smoltcp::phy::{Device, RxToken, TxToken};
use smoltcp::socket::tcp::{Socket, SocketBuffer, State};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpEndpoint};
use std::cell::{LazyCell, OnceCell};
use std::io::Write;
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::{thread, time::Duration};

// Test constants
const INTERFACE_NAME1: &str = "test_iface1";
const INTERFACE_MAC2: &str = "2a:2b:72:fb:e8:cc";
const INTERFACE_IP1: &str = "192.168.10.1";

const INTERFACE_NAME2: &str = "test_iface2";
const INTERFACE_MAC1: &str = "f2:ef:e0:30:6a:7f";
const INTERFACE_IP2: &str = "192.168.10.2";

static SETUP: LazyLock<()> = LazyLock::new(|| {
    let status = Command::new("nu")
        .arg("setup_net.nu")
        .status()
        .expect("Failed to execute setup_net.nu script");

    assert!(status.success(), "Network setup failed");

    // Allow some time for interfaces to be fully ready
    thread::sleep(Duration::from_secs(1));
});

static CLEANUP: LazyLock<()> = LazyLock::new(|| {
    let status = Command::new("nu")
        .arg("cleanup_net.nu")
        .status()
        .expect("Failed to execute cleanup_net.nu script");

    assert!(status.success(), "Network cleanup failed");
});

#[test]
fn test_xdp_device_send_receive() {
    *SETUP;

    let mut device1: XdpDevice<1024> =
        XdpDevice::new(INTERFACE_NAME1).expect("Failed to create XDP device");
    let xsk_mac1 = INTERFACE_MAC1
        .parse::<EthernetAddress>()
        .expect("Failed to parse MAC address");
    let mut iface1 = Interface::new(
        smoltcp::iface::Config::new(xsk_mac1.into()),
        &mut device1,
        Instant::now(),
    );
    let socket1 = Socket::new(
        SocketBuffer::new(vec![0; 4 * 1024]),
        SocketBuffer::new(vec![0; 4 * 1024]),
    );
    let mut sockets1 = SocketSet::new(vec![]);
    let handle1 = sockets1.add(socket1);

    let mut device2: XdpDevice<1024> =
        XdpDevice::new(INTERFACE_NAME2).expect("Failed to create XDP device");
    let xsk_mac2 = INTERFACE_MAC2
        .parse::<EthernetAddress>()
        .expect("Failed to parse MAC address");
    let mut iface2 = Interface::new(
        smoltcp::iface::Config::new(xsk_mac2.into()),
        &mut device2,
        Instant::now(),
    );
    let socket2 = Socket::new(
        SocketBuffer::new(vec![0; 4 * 1024]),
        SocketBuffer::new(vec![0; 4 * 1024]),
    );
    let mut sockets2 = SocketSet::new(vec![]);
    let handle2 = sockets2.add(socket2);

    let server_endpoint = IpEndpoint {
        addr: smoltcp::wire::IpAddress::from_str(INTERFACE_IP1).unwrap(),
        port: 443,
    };

    let socket1 = sockets1.get_mut::<Socket>(handle1);
    // socket1 开始监听
    socket1.listen(server_endpoint).unwrap();

    iface1.poll(Instant::now(), &mut device1, &mut sockets1);
    let socket1 = sockets1.get_mut::<Socket>(handle1);
    assert!(socket1.state() == State::Listen);

    let socket2 = sockets2.get_mut::<Socket>(handle2);
    socket2
        .connect(iface1.context(), server_endpoint, 443)
        .unwrap();

    // socket2 发送 SYN 请求建立连接
    iface2.poll(Instant::now(), &mut device2, &mut sockets2);
    let socket2 = sockets2.get_mut::<Socket>(handle2);
    assert!(socket2.state() == State::SynSent);

    // socket1 返回 SYN+ACK 接受连接
    iface1.poll(Instant::now(), &mut device1, &mut sockets1);
    let socket1 = sockets1.get_mut::<Socket>(handle1);
    assert!(socket1.state() == State::SynReceived);

    // socket2 返回 ACK 确认连接
    iface2.poll(Instant::now(), &mut device2, &mut sockets2);
    let socket2 = sockets2.get_mut::<Socket>(handle2);
    assert!(socket2.state() == State::SynSent);

    // socket1 完成连接
    iface1.poll(Instant::now(), &mut device1, &mut sockets1);
    let socket1 = sockets1.get_mut::<Socket>(handle1);

    assert!(socket1.state() == State::Established);
    assert!(socket2.state() == State::Established);

    *CLEANUP;
}
