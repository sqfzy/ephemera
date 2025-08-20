use eyre::{ContextCompat, Result};
use smoltcp::{
    phy::{Checksum, Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use std::{
    fmt::Debug,
    io,
    mem::MaybeUninit,
    net::Ipv4Addr,
    num::NonZeroU32,
    ops::Range,
    os::fd::{AsRawFd, RawFd},
};
use tracing::{debug, trace};
use xsk_rs::{
    CompQueue, FillQueue, FrameDesc,
    config::{LibxdpFlags, SocketConfig, UmemConfig, XdpFlags},
    socket::{RxQueue, Socket as XskSocket, TxQueue},
    umem::Umem,
};

use crate::bpf::xdp_ip_filter::XdpIpFilter;

#[derive(Debug)]
pub struct XdpDevice {
    rx_q: RxQueue,
    rx_fds: Vec<FrameDesc>,
    rx_fds_can_read: Range<usize>,

    tx_q: TxQueue,
    tx_fds: Vec<FrameDesc>,
    tx_fds_can_send: Range<usize>,

    // 在接收数据前，需要向内核produce fd
    fq: FillQueue,
    // 内核处理完要发送的数据后，需要consume数据对应的fd
    cq: CompQueue,
    umem: Umem,

    fd: RawFd,
}

impl XdpDevice {
    /// Create a new XDP reactor for the given interface name.
    ///
    /// # Example
    ///
    /// ```
    /// let device = XdpDevice::new("interface_name_foo").unwrap();
    /// let mac = "00:11:22:33:44:55".parse::<smoltcp::wire::EthernetAddress>().unwrap();
    /// let reactor = XdpReactor::new(device, mac);
    ///
    ///
    /// // Usually, you want to update the iface.
    /// reactor.iface.update_ip_addrs(|ip_addrs| {
    ///     ip_addrs
    ///         .push(IpCidr::new(
    ///             your_ip_addr.parse::<Ipv4Addr>().unwrap().into(),
    ///             24,
    ///         ))
    ///         .unwrap();
    /// });
    /// reactor
    ///     .iface
    ///     .routes_mut()
    ///     .add_default_ipv4_route(your_gateway_ip)
    ///     .expect("Failed to add default route");
    /// ```
    pub fn new(if_name: &str) -> Result<Self> {
        // INFO: 目前XDP Device 只实现了单个网卡队列的处理逻辑，并指定处理的队列ID为0
        const QUEUE_ID: u32 = 0;

        let if_name_parsed = if_name.parse()?;
        let frame_count = 1024_u32;

        // PERF: huge page
        let (umem, descs) = Umem::new(
            UmemConfig::default(),
            NonZeroU32::new(frame_count).wrap_err("FRAME_COUNT must be non-zero")?,
            false,
        )?;

        // PERF:
        let sk_conf = SocketConfig::builder()
            // .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE) // 使用 SKB 模式
            .libxdp_flags(LibxdpFlags::XSK_LIBXDP_FLAGS_INHIBIT_PROG_LOAD) // 不要使用默认的XDP程序
            .build();

        // TODO: 使用相同的if_name创建多个XskSocket是不安全的，需要封装来避免
        // FIX: xsks_map key=0时，value!=if_index。需修正
        let (tx_q, rx_q, fq_and_cq) =
            unsafe { XskSocket::new(sk_conf, &umem, &if_name_parsed, QUEUE_ID) }?;

        let (mut fq, cq) = fq_and_cq.expect("UMEM is not shared, fq and cq should exist");

        // 填充 Fill Queue，让内核有可用的帧来接收数据
        unsafe { fq.produce(&descs) };

        Ok(Self {
            fd: tx_q.fd().as_raw_fd(),
            rx_q,
            rx_fds: vec![FrameDesc::default(); frame_count as usize],
            rx_fds_can_read: 0..0,
            tx_q,
            tx_fds: vec![FrameDesc::default(); frame_count as usize],
            tx_fds_can_send: 0..frame_count as usize,
            fq,
            cq,
            umem,
        })
    }

    pub fn get_fd_can_read(&mut self) -> Option<FrameDesc> {
        debug_assert!(self.rx_fds_can_read.start <= self.rx_fds_can_read.end);

        // Example: 初始rx_fds_can_read为空
        //
        // 内核可读: fd1, fd2, fd3, fd4, fd5
        // 已有数据: fd1, fd2, fd3
        // 用户已用：fd1
        //
        // 我们要记录fd2,fd3，它们可供用户读取数据。当fd1,fd2,fd3都已被
        // 用户使用，则重新向内核提供，让内核可以继续消费它们。
        //
        // 我们关心哪些fd是用户能读的，并希望用户能尽快读取。

        // 用户用完了所有已读到数据的fd
        if self.rx_fds_can_read.start == self.rx_fds_can_read.end {
            // 向内核提供这些fd，内核会消费它们来读数据
            unsafe { self.fq.produce(&self.rx_fds[..self.rx_fds_can_read.end]) };

            // 消费内核提供的fd个数，证明这些fd已经读到了数据
            let n = unsafe { self.rx_q.consume(&mut self.rx_fds) };

            self.rx_fds_can_read = 0..n;
        }

        if self.rx_fds_can_read.start == self.rx_fds_can_read.end {
            // 没有可读的fd
            return None;
        }

        let rx_fd = self.rx_fds[self.rx_fds_can_read.start];
        self.rx_fds_can_read.start += 1;

        Some(rx_fd)
    }

    pub fn get_fd_can_send(&mut self) -> Option<FrameDesc> {
        debug_assert!(self.tx_fds_can_send.start <= self.tx_fds_can_send.end);

        // Example: 初始tx_fds_can_read为所有fd
        //
        // 内核需发: fd1..fdN
        // 已发送: fd1, fd2, fd3
        //
        // 当且仅当所有fd都处于待发送状态时，检查哪些fd已经发送，重新
        // 获取可用的fd。
        //
        // 我们只关心有没有能用来发数据的fd，具体的发送操作有内核负责。

        // 用户用完了所有可用来发送数据的fd
        if self.tx_fds_can_send.start == self.tx_fds_can_send.end {
            // 消费内核提供的fd个数，证明这些fd已经发送了数据
            let n = unsafe { self.cq.consume(&mut self.tx_fds) };

            self.tx_fds_can_send = 0..n;
        }

        if self.tx_fds_can_send.start == self.tx_fds_can_send.end {
            // 没有可用的fd
            return None;
        }

        let tx_fd = self.tx_fds[self.tx_fds_can_send.start];
        self.tx_fds_can_send.start += 1;

        Some(tx_fd)
    }

    pub fn wakeup_kernel(&mut self) -> io::Result<()> {
        // 唤醒内核，通知它有数据可以发送
        self.tx_q.wakeup()
    }
}

impl AsRawFd for XdpDevice {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl Device for XdpDevice {
    type RxToken<'token>
        = XskRxToken<'token>
    where
        Self: 'token;
    type TxToken<'token>
        = XskTxToken<'token>
    where
        Self: 'token;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let fd_read = self.get_fd_can_read()?;
        let fd_send = self.get_fd_can_send()?;

        let rx_token = XskRxToken {
            umem: &self.umem,
            fd: fd_read,
        };
        let tx_token = XskTxToken {
            umem: &self.umem,
            fd: fd_send,
            tx_q: &mut self.tx_q,
        };

        Some((rx_token, tx_token))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(XskTxToken {
            fd: self.get_fd_can_send()?,
            umem: &self.umem,
            tx_q: &mut self.tx_q,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        // caps.checksum.ipv4 = Checksum::Tx;
        // caps.checksum.tcp = Checksum::Tx;
        caps
    }
}

pub struct XskRxToken<'a> {
    umem: &'a Umem,
    // 指向umem某个frame
    fd: FrameDesc,
}

impl<'a> RxToken for XskRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let data = unsafe { self.umem.data(&self.fd) };

        trace!("xdp recv: {:?}", data.contents());

        f(data.contents())
    }
}

/// When this token is consum, the kernel will not be awakened.
/// It is up to the user to decide when to wake it up (by calling [' wakeup kernel ']).
pub struct XskTxToken<'a> {
    umem: &'a Umem,
    // 指向umem某个frame
    fd: FrameDesc,
    tx_q: &'a mut TxQueue,
}

impl<'a> TxToken for XskTxToken<'a> {
    fn consume<R, F>(mut self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // 从 UMEM 获取整个帧的可变数据区域
        let mut data_mut = unsafe { self.umem.data_mut(&mut self.fd) };
        let mut cursor = data_mut.cursor();

        assert!(
            len <= cursor.buf_len(),
            "Requested length {} exceeds available buffer length {}",
            len,
            cursor.buf_len()
        );

        // smoltcp requires the buffer length must be `len`
        cursor.set_pos(len);

        let result = f(data_mut.contents_mut());

        trace!("xdp send: {:?}", data_mut.contents());

        unsafe { self.tx_q.produce_one(&self.fd) };

        result
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use crate::af_xdp::device::XdpDevice;
    use crate::test_utils::*;
    use smoltcp::{
        phy::{Device, RxToken, TxToken},
        time::Instant,
        wire::{EthernetFrame, EthernetProtocol},
    };

    #[test]
    fn test_device_send_and_recv() {
        setup();

        let mut device1 = XdpDevice::new(INTERFACE_NAME1).unwrap();
        let mut device2 = XdpDevice::new(INTERFACE_NAME2).unwrap();

        let mac1 = INTERFACE_MAC1
            .parse::<smoltcp::wire::EthernetAddress>()
            .unwrap();
        let mac2 = INTERFACE_MAC2
            .parse::<smoltcp::wire::EthernetAddress>()
            .unwrap();

        // --- device1 发送消息 ---
        {
            let tx_token = device1.transmit(Instant::now()).unwrap();
            let msg = b"Hello XDP!";
            let frame_len = EthernetFrame::<&[u8]>::header_len() + msg.len();

            tx_token.consume(frame_len, |mut frame_buf| {
                // 在缓冲区上构建一个以太网帧
                let mut frame = EthernetFrame::new_unchecked(&mut frame_buf);
                frame.set_dst_addr(mac2);
                frame.set_src_addr(mac1);
                // 指定上层协议
                frame.set_ethertype(EthernetProtocol::Ipv4);
                // 将消息内容复制到帧的负载部分
                frame.payload_mut().copy_from_slice(msg);
            });

            // 唤醒内核处理发送队列
            device1.wakeup_kernel().unwrap();
        }

        // --- device2 接收并回复消息 ---
        {
            let (rx_token, tx_token) = device2.receive(Instant::now()).unwrap();
            // 1. 接收和验证
            rx_token.consume(|frame_buf| {
                let frame = EthernetFrame::new_checked(frame_buf).unwrap();
                // 验证MAC地址和负载内容
                assert_eq!(frame.src_addr(), mac1);
                assert_eq!(frame.dst_addr(), mac2);
                assert_eq!(frame.payload(), b"Hello XDP!");
            });

            // 2. 准备并发送回复
            let reply_msg = b"Hi!";
            let reply_frame_len = EthernetFrame::<&[u8]>::header_len() + reply_msg.len();

            tx_token.consume(reply_frame_len, |mut frame_buf| {
                let mut frame = EthernetFrame::new_unchecked(&mut frame_buf);
                frame.set_src_addr(mac2);
                frame.set_dst_addr(mac1);
                frame.set_ethertype(EthernetProtocol::Ipv4);
                frame.payload_mut().copy_from_slice(reply_msg);
            });

            // 唤醒内核处理发送队列
            device2.wakeup_kernel().unwrap();
        }

        // --- device1 接收回复 ---
        {
            let (rx_token, _) = device1.receive(Instant::now()).unwrap();
            rx_token.consume(|frame_buf| {
                let frame = EthernetFrame::new_checked(frame_buf).unwrap();
                assert_eq!(frame.src_addr(), mac2);
                assert_eq!(frame.dst_addr(), mac1);
                assert_eq!(frame.payload(), b"Hi!");
            });
        }
    }

    #[test]
    fn test_device_send_and_recv2() {
        setup();

        let mut device1 = XdpDevice::new(INTERFACE_NAME1).unwrap();
        let mut device2 = XdpDevice::new(INTERFACE_NAME2).unwrap();

        let mac1 = INTERFACE_MAC1
            .parse::<smoltcp::wire::EthernetAddress>()
            .unwrap();
        let mac2 = INTERFACE_MAC2
            .parse::<smoltcp::wire::EthernetAddress>()
            .unwrap();

        // --- device1 发送消息 ---
        {
            let tx_token = device1.transmit(Instant::now()).unwrap();
            let msg = b"Hello XDP!";
            let frame_len = EthernetFrame::<&[u8]>::header_len() + msg.len();

            tx_token.consume(frame_len, |mut frame_buf| {
                // 在缓冲区上构建一个以太网帧
                let mut frame = EthernetFrame::new_unchecked(&mut frame_buf);
                frame.set_dst_addr(mac2);
                frame.set_src_addr(mac1);
                // 指定上层协议
                frame.set_ethertype(EthernetProtocol::Ipv4);
                // 将消息内容复制到帧的负载部分
                frame.payload_mut().copy_from_slice(msg);
            });

            // 唤醒内核处理发送队列
            device1.wakeup_kernel().unwrap();
        }

        {
            let tx_token = device1.transmit(Instant::now()).unwrap();
            let msg = b"Hello XDP2!";
            let frame_len = EthernetFrame::<&[u8]>::header_len() + msg.len();

            tx_token.consume(frame_len, |mut frame_buf| {
                // 在缓冲区上构建一个以太网帧
                let mut frame = EthernetFrame::new_unchecked(&mut frame_buf);
                frame.set_dst_addr(mac2);
                frame.set_src_addr(mac1);
                // 指定上层协议
                frame.set_ethertype(EthernetProtocol::Ipv4);
                // 将消息内容复制到帧的负载部分
                frame.payload_mut().copy_from_slice(msg);
            });

            // 唤醒内核处理发送队列
            device1.wakeup_kernel().unwrap();
        }

        // --- device2 接收并回复消息 ---
        {
            let (rx_token, tx_token) = device2.receive(Instant::now()).unwrap();
            // 1. 接收和验证
            rx_token.consume(|frame_buf| {
                let frame = EthernetFrame::new_checked(frame_buf).unwrap();
                // 验证MAC地址和负载内容
                assert_eq!(frame.src_addr(), mac1);
                assert_eq!(frame.dst_addr(), mac2);
                assert_eq!(frame.payload(), b"Hello XDP!");
            });
            let (rx_token, tx_token) = device2.receive(Instant::now()).unwrap();
            rx_token.consume(|frame_buf| {
                let frame = EthernetFrame::new_checked(frame_buf).unwrap();
                // 验证MAC地址和负载内容
                assert_eq!(frame.src_addr(), mac1);
                assert_eq!(frame.dst_addr(), mac2);
                assert_eq!(frame.payload(), b"Hello XDP2!");
            });

            // 2. 准备并发送回复
            let reply_msg = b"Hi!";
            let reply_frame_len = EthernetFrame::<&[u8]>::header_len() + reply_msg.len();

            tx_token.consume(reply_frame_len, |mut frame_buf| {
                let mut frame = EthernetFrame::new_unchecked(&mut frame_buf);
                frame.set_src_addr(mac2);
                frame.set_dst_addr(mac1);
                frame.set_ethertype(EthernetProtocol::Ipv4);
                frame.payload_mut().copy_from_slice(reply_msg);
            });

            // 唤醒内核处理发送队列
            device2.wakeup_kernel().unwrap();
        }

        // --- device1 接收回复 ---
        {
            let (rx_token, _) = device1.receive(Instant::now()).unwrap();
            rx_token.consume(|frame_buf| {
                let frame = EthernetFrame::new_checked(frame_buf).unwrap();
                assert_eq!(frame.src_addr(), mac2);
                assert_eq!(frame.dst_addr(), mac1);
                assert_eq!(frame.payload(), b"Hi!");
            });
        }
    }
}
