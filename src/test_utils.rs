use std::process::Command;
use std::str::FromStr;
use std::sync::{LazyLock, Mutex, OnceLock};

use xsk_rs::config::{SocketConfig, XdpFlags};

use crate::af_xdp::device::XdpDevice;

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

// use futures::try_join;
// use rtnetlink::new_connection;
// use std::net::{IpAddr, Ipv4Addr};
//
// pub const IF_NAME_ROOT: &str = "veth-test1";
// pub const IF_NAME_NS: &str = "veth-test2";
// pub const NS_NAME: &str = "ns-test";
// pub const IP_ROOT: Ipv4Addr = Ipv4Addr::new(192, 168, 10, 1);
// pub const IP_NS: Ipv4Addr = Ipv4Addr::new(192, 168, 10, 2);
// pub const PREFIX_LEN: u8 = 24;
//
// pub async fn setup_veth_environment() -> Result<(), rtnetlink::Error> {
//     // 1. 获取到 netlink 的连接和句柄
//     let (connection, handle, _) = new_connection()?;
//     // 必须 spawn 一个后台任务来处理 netlink 消息
//     tokio::spawn(connection);
//
//     // 2. 创建 veth pair
//     handle
//         .link()
//         .add()
//         .veth(IF_NAME_ROOT.into(), IF_NAME_NS.into())
//         .execute()
//         .await?;
//     println!("Created veth pair: {} <--> {}", IF_NAME_ROOT, IF_NAME_NS);
//
//     // 3. 创建网络命名空间
//     handle.netns().add(NS_NAME.into()).execute().await?;
//     println!("Created network namespace: {}", NS_NAME);
//
//     // 4. 将 veth 的一端移动到新的命名空间
//     let link_ns_index = handle
//         .link()
//         .get()
//         .match_name(IF_NAME_NS.into())
//         .execute()
//         .await?
//         .first()
//         .unwrap()
//         .header
//         .index;
//     handle
//         .link()
//         .set(link_ns_index)
//         .set_ns_by_name(NS_NAME.into())
//         .execute()
//         .await?;
//     println!("Moved {} into namespace {}", IF_NAME_NS, NS_NAME);
//
//     // 5. 配置网络接口 (这是最复杂的部分)
//     //    需要为新的命名空间创建一个新的 netlink handle
//     let ns_handle = handle.netns().open(NS_NAME.into()).await?;
//
//     // 并行配置两个接口
//     try_join!(
//         // a. 配置 root 命名空间中的 veth-test1
//         async {
//             let link_root_index = handle
//                 .link()
//                 .get()
//                 .match_name(IF_NAME_ROOT.into())
//                 .execute()
//                 .await?
//                 .first()
//                 .unwrap()
//                 .header
//                 .index;
//             handle
//                 .address()
//                 .add(link_root_index, IpAddr::V4(IP_ROOT), PREFIX_LEN)
//                 .execute()
//                 .await?;
//             handle.link().set(link_root_index).up().execute().await?;
//             println!(
//                 "Configured {} with IP {}/{}",
//                 IF_NAME_ROOT, IP_ROOT, PREFIX_LEN
//             );
//             Ok(())
//         },
//         // b. 配置 ns-test 命名空间中的 veth-test2
//         async {
//             // 注意：这里我们使用 ns_handle
//             let link_ns_index = ns_handle
//                 .link()
//                 .get()
//                 .match_name(IF_NAME_NS.into())
//                 .execute()
//                 .await?
//                 .first()
//                 .unwrap()
//                 .header
//                 .index;
//             ns_handle
//                 .address()
//                 .add(link_ns_index, IpAddr::V4(IP_NS), PREFIX_LEN)
//                 .execute()
//                 .await?;
//             ns_handle.link().set(link_ns_index).up().execute().await?;
//             println!("Configured {} with IP {}/{}", IF_NAME_NS, IP_NS, PREFIX_LEN);
//             Ok(())
//         }
//     )?;
//
//     Ok(())
// }
//
// pub async fn teardown_veth_environment() -> Result<(), rtnetlink::Error> {
//     let (connection, handle, _) = new_connection()?;
//     tokio::spawn(connection);
//
//     // 只需删除网络命名空间，与之关联的 veth 设备也会被自动删除
//     // ip link del veth-test1 是可选的，因为 veth 的另一端消失后它也会消失
//     match handle.netns().delete(NS_NAME.into()).execute().await {
//         Ok(_) => println!("Deleted namespace {}", NS_NAME),
//         Err(rtnetlink::Error::NetlinkError(e)) if e.code == -2 => {
//             // -2 is ENOENT (No such file or directory), meaning the ns was already gone.
//             println!("Namespace {} already deleted.", NS_NAME);
//         }
//         Err(e) => return Err(e),
//     }
//
//     Ok(())
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     // 使用 #[tokio::test] 来运行异步测试
//     #[tokio::test]
//     #[serial_test::serial(net_env)] // 假设你继续使用 serial_test
//     async fn test_environment_setup_and_teardown() {
//         println!("--- Setting up environment ---");
//         setup_veth_environment().await.unwrap();
//
//         // 在这里可以添加一些 ping 测试或者其他检查来验证环境是否正确
//         // ...
//
//         println!("--- Tearing down environment ---");
//         teardown_veth_environment().await.unwrap();
//     }
// }
