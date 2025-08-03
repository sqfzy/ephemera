use ephemera::af_xdp::device::XdpDevice;
use ephemera::af_xdp::reactor::XdpReactor;
use ephemera::af_xdp::stream::XdpTcpStreamBorrow;
use portpicker::pick_unused_port;
use smoltcp::socket::tcp::{Socket, SocketBuffer, State};
use smoltcp::time::Duration;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint};
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
    env_logger::builder()
        .filter_level(log::LevelFilter::Trace)
        .init();

    Command::new("sudo")
        .arg("nu")
        .arg("tests/setup_net.nu")
        .output()
        .expect("Failed to execute setup_net.nu script");

    SetupGuard {}
});

#[test]
fn test_xdp_device_send_receive() {
    let _ = *SETUP;

    let mac_addr1 = INTERFACE_MAC1.parse::<EthernetAddress>().unwrap();
    let ip_addr1 = IpAddress::from_str(INTERFACE_IP1).unwrap();

    let device1: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME1).unwrap();

    let mut reactor1 = XdpReactor::new(device1, mac_addr1);
    reactor1.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr1, 24)).unwrap();
    });
    let handle1 = reactor1.sockets.add(Socket::new(
        SocketBuffer::new(vec![0; 4 * 1024]),
        SocketBuffer::new(vec![0; 4 * 1024]),
    ));

    let mac_addr2 = INTERFACE_MAC2.parse::<EthernetAddress>().unwrap();
    let ip_addr2 = IpAddress::from_str(INTERFACE_IP2).unwrap();

    let device2: XdpDevice<1024> = XdpDevice::new(INTERFACE_NAME2).unwrap();

    let mut reactor2 = XdpReactor::new(device2, mac_addr2);
    reactor2.iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(ip_addr2, 24)).unwrap();
    });
    let stream2 = XdpTcpStreamBorrow::connect(addr, &mut reactor2);

    let server_endpoint = IpEndpoint::new(ip_addr1, 443);

    // Server 开始监听
    reactor1
        .socket_mut(handle1)
        .listen(server_endpoint)
        .unwrap();
    assert_eq!(reactor1.socket_mut(handle1).state(), State::Listen);

    // Client 发起连接请求
    let local_port = pick_unused_port().unwrap();
    reactor2
        .socket_mut(handle2)
        .connect(reactor2.iface.context(), server_endpoint, local_port)
        .unwrap();
    assert_eq!(reactor2.socket_mut(handle2).state(), State::SynSent);

    // 模拟网络交互
    for _ in 0..100 {
        reactor1.poll(Some(Duration::ZERO)).unwrap();
        reactor2.poll(Some(Duration::ZERO)).unwrap();

        // 成功建立连接后退出循环
        if reactor1.socket_mut(handle1).state() == State::Established
            && reactor2.socket_mut(handle2).state() == State::Established
        {
            break;
        }
    }

    assert_eq!(reactor1.socket_mut(handle1).state(), State::Established);
    assert_eq!(reactor2.socket_mut(handle2).state(), State::Established);

}
