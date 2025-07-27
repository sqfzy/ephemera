use eyre::{ContextCompat, Result};
use smoltcp::{
    phy::{Checksum, Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use std::{
    fmt::Debug,
    io,
    num::NonZeroU32,
    ops::Range,
    os::fd::{AsRawFd, RawFd},
};
use xsk_rs::{
    CompQueue, FillQueue, FrameDesc,
    config::{SocketConfig, UmemConfig, XdpFlags},
    socket::{RxQueue, Socket as XskSocket, TxQueue},
    umem::Umem,
};

#[derive(Debug)]
pub struct XdpDevice<const FRAME_COUNT: usize = 1024> {
    rx_q: RxQueue,
    rx_fds: [FrameDesc; FRAME_COUNT],
    rx_fds_can_read: Range<usize>,

    tx_q: TxQueue,
    tx_fds: [FrameDesc; FRAME_COUNT],
    tx_fds_can_send: Range<usize>,

    // 在接收数据前，需要向内核produce fd
    fq: FillQueue,
    // 内核处理完要发送的数据后，需要consume数据对应的fd
    cq: CompQueue,
    umem: Umem,

    fd: RawFd,
}

impl<const FRAME_COUNT: usize> XdpDevice<FRAME_COUNT> {
    pub fn new(if_name: &str) -> Result<Self> {
        let if_name_parsed = if_name.parse().unwrap();

        let frame_count =
            NonZeroU32::new(FRAME_COUNT as u32).wrap_err("FRAME_COUNT must be non-zero")?;
        let (umem, descs) = Umem::new(UmemConfig::default(), frame_count, true)?;

        let socket_conf = SocketConfig::builder()
            .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE)
            .build();

        let (tx_q, rx_q, fq_and_cq) =
            unsafe { XskSocket::new(socket_conf, &umem, &if_name_parsed, 0) }?;

        let (mut fq, cq) = fq_and_cq.expect("UMEM is not shared, fq and cq should exist");

        // 填充 Fill Queue，让内核有可用的帧来接收数据
        unsafe { fq.produce(&descs) };

        Ok(Self {
            fd: tx_q.fd().as_raw_fd(),
            rx_q,
            rx_fds: [FrameDesc::default(); FRAME_COUNT],
            rx_fds_can_read: 0..0,
            tx_q,
            tx_fds: [FrameDesc::default(); FRAME_COUNT],
            tx_fds_can_send: 0..FRAME_COUNT,
            fq,
            cq,
            umem,
        })
    }

    fn get_fd_can_read(&mut self) -> Option<FrameDesc> {
        debug_assert!(self.rx_fds_can_read.start <= self.rx_fds_can_read.end);

        // Example: 初始rx_fds_can_read为空
        //
        // 内核可读: fd1, fd2, fd3, fd4, fd5
        // 已有数据: fd1, fd2, fd3
        // 用户已用：fd1
        //
        // 我们要记录fd2,fd3，让用户可以继续读取数据。当fd1,fd2,fd3都已被
        // 用户使用，则重新向内核提供，让内核可以继续消费它们。
        //
        // 我们关心哪些fd是用户能读的，并希望用户能尽快读取。

        // 用户用完了所有已读到数据的fd
        if self.rx_fds_can_read.start == self.rx_fds_can_read.end {
            // 向内核提供这些fd，让内核会消费它们来读数据
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

    fn get_fd_can_send(&mut self) -> Option<FrameDesc> {
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

impl<const FRAME_COUNT: usize> AsRawFd for XdpDevice<FRAME_COUNT> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl<const FRAME_COUNT: usize> Device for XdpDevice<FRAME_COUNT> {
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
        caps.checksum.ipv4 = Checksum::Tx;
        caps.checksum.tcp = Checksum::Tx;
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
        f(unsafe { self.umem.data(&self.fd).contents() })
    }
}

pub struct XskTxToken<'a> {
    umem: &'a Umem,
    // 指向umem某个frame
    fd: FrameDesc,
    tx_q: &'a mut TxQueue,
}

impl<'a> TxToken for XskTxToken<'a> {
    fn consume<R, F>(mut self, _len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // TODO: len可能大于data的容量?

        let mut data_mut = unsafe { self.umem.data_mut(&mut self.fd) };
        let result = f(data_mut.contents_mut());

        unsafe { self.tx_q.produce_one(&self.fd) };

        result
    }
}
