use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use std::{
    error::Error,
    fmt::Debug,
    io,
    num::NonZeroU32,
    os::fd::{AsRawFd, RawFd},
};
use tracing::trace;
use xsk_rs::{
    CompQueue, FillQueue, FrameDesc,
    config::{LibxdpFlags, SocketConfig, UmemConfig},
    socket::{RxQueue, Socket as XskSocket, TxQueue},
    umem::Umem,
};

#[derive(Debug)]
pub(crate) struct XdpDevice<const FC: usize = 1024> {
    pub(crate) reader: XdpReader<FC>,
    pub(crate) writer: XdpWriter<FC>,
    pub(crate) umem: Umem,
    pub(crate) fd: RawFd,
}

impl<const FC: usize> XdpDevice<FC> {
    pub(crate) fn new(if_name: &str) -> Result<Self, Box<dyn Error>> {
        // PERF:
        let sk_conf = SocketConfig::builder()
            // .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE) // 使用 SKB 模式
            .libxdp_flags(LibxdpFlags::XSK_LIBXDP_FLAGS_INHIBIT_PROG_LOAD) // 不要使用默认的XDP程序
            .build();

        Self::new_with_config(if_name, sk_conf)
    }

    pub(crate) fn new_with_config(
        if_name: &str,
        sk_conf: SocketConfig,
    ) -> Result<Self, Box<dyn Error>> {
        // INFO: 目前XDP Device 只实现了单个网卡队列的处理逻辑，并指定处理的队列ID为0
        const QUEUE_ID: u32 = 0;

        let if_name_parsed = if_name.parse()?;
        let frame_count = (FC * 2) as u32;

        // PERF: huge page
        let (umem, descs) = Umem::new(
            UmemConfig::default(),
            NonZeroU32::new(frame_count).expect("FRAME_COUNT must be non-zero"),
            false,
        )?;
        let rx_fds: [FrameDesc; FC] = descs[..FC].try_into().unwrap();
        let tx_fds: [FrameDesc; FC] = descs[FC..].try_into().unwrap();

        // TODO: 使用相同的if_name创建多个XskSocket是不安全的，需要封装来避免
        let (tx_q, rx_q, fq_and_cq) =
            unsafe { XskSocket::new(sk_conf, &umem, &if_name_parsed, QUEUE_ID) }?;

        let (mut fq, cq) = fq_and_cq.expect("UMEM is not shared, fq and cq should exist");

        unsafe {
            // 初始化，所有rx_fds，内核都可用来读
            fq.produce(&rx_fds);
        };

        Ok(Self {
            fd: tx_q.fd().as_raw_fd(),
            umem,
            reader: XdpReader {
                rx_q,
                rx_fds,
                fq,
                kernel_can_write_pos: 0,
                user_can_recv_pos: 0,
                user_has_recv_pos: 0,
                user_can_recv_len: 0,
                user_has_recv_len: 0,
            },
            writer: XdpWriter {
                tx_q,
                tx_fds,
                cq,
                user_can_write_pos: 0,
                user_has_write_pos: 0,
                kernel_has_send_pos: 0,
                user_has_write_len: 0,
                kernel_has_send_len: 0,
            },
        })
    }

    pub(crate) fn flush(&mut self) -> io::Result<usize> {
        self.writer.user_produce_and_wakeup()
    }
}

/// ```txt
/// |____user_has_recv____|____user_can_recv____|____kernel_can_write____
/// ```
#[derive(Debug)]
pub(crate) struct XdpReader<const FC: usize> {
    pub(crate) rx_q: RxQueue,
    pub(crate) rx_fds: [FrameDesc; FC],
    // 用户在接收数据前，需要向内核produce fd
    pub(crate) fq: FillQueue,

    /// “内核可写”区的起始点。也是“用户可接收”区的终点。
    /// 内核可写意味着内核可用该 fd 来接收数据。
    kernel_can_write_pos: usize,

    /// “用户可接收”区的起始点。也是“用户已接收”区的终点。
    user_can_recv_pos: usize,

    /// “用户已接收”区的起始点。也是“内核可写”区的终点。
    user_has_recv_pos: usize,

    // 为了区分 empty 和 full，引入长度
    //
    /// “用户可接收” -> “内核可写”
    user_can_recv_len: usize,

    /// “内核可写” -> “用户已接收”
    user_has_recv_len: usize,
}

impl<const FC: usize> XdpReader<FC> {
    /// “内核可写”区域的长度 (空闲空间，待内核写入)
    pub(crate) const fn kernel_can_write_len(&self) -> usize {
        self.rx_fds.len() - self.user_can_recv_len - self.user_has_recv_len
    }

    /// “用户可读”区域的长度 (已有数据，待用户读取)
    pub(crate) const fn user_can_recv_len(&self) -> usize {
        self.user_can_recv_len
    }

    /// “用户已接收”区域的长度 (无效数据，待内核回收)
    pub(crate) const fn user_has_recv_len(&self) -> usize {
        // CHANGED: 现在直接返回存储的字段
        self.user_has_recv_len
    }

    /// 用户向内核提供用户已经用完的所有 fd。
    /// “用户已接收”区域 -> “内核可写”区域
    pub(crate) fn user_produce(&mut self) -> usize {
        let mut n_produce = 0;

        // 用户已接收区
        let (s1, s2) = advance_get_mut(
            self.user_has_recv_len(),
            self.user_has_recv_pos,
            self.user_can_recv_pos,
            &mut self.rx_fds,
        );

        if !s1.is_empty() {
            let n = unsafe { self.fq.produce(s1) };
            // kernel_can_write_len update automatically
            self.user_has_recv_len -= n;
            self.user_has_recv_pos = advance(self.user_has_recv_pos, n, FC);

            n_produce += n;
            if n != s1.len() {
                return n_produce;
            }
        }

        if !s2.is_empty() {
            let n = unsafe { self.fq.produce(s2) };
            // kernel_can_write_len update automatically
            self.user_has_recv_len -= n;
            self.user_has_recv_pos = advance(self.user_has_recv_pos, n, FC);

            n_produce += n;
            if n != s2.len() {
                return n_produce;
            }
        }

        debug_assert_eq!(
            self.user_has_recv_len, 0,
            "user_has_recv_len should be zero after producing all"
        );

        n_produce
    }

    /// 用户尝试获得已经被内核写入数据的 fd。
    /// “内核可写”区域 -> “用户可接收”区域
    pub(crate) fn user_consume(&mut self) -> usize {
        let mut n_consume = 0;

        // 内核可写区
        let (s1, s2) = advance_get_mut(
            self.kernel_can_write_len(),
            self.kernel_can_write_pos,
            self.user_has_recv_pos,
            &mut self.rx_fds,
        );

        if !s1.is_empty() {
            let n = unsafe { self.rx_q.consume(s1) };
            // kernel_can_write_len update automatically
            self.user_can_recv_len += n;
            self.kernel_can_write_pos = advance(self.kernel_can_write_pos, n, FC);

            n_consume += n;
            if n != s1.len() {
                return n_consume;
            }
        }

        if !s2.is_empty() {
            let n = unsafe { self.rx_q.consume(s2) };
            // kernel_can_write_len update automatically
            self.user_can_recv_len += n;
            self.kernel_can_write_pos = advance(self.kernel_can_write_pos, n, FC);

            n_consume += n;
            if n != s2.len() {
                return n_consume;
            }
        }

        debug_assert_eq!(
            self.kernel_can_write_len(),
            0,
            "user_can_recv_len should be zero after consuming all"
        );

        n_consume
    }

    /// 用户读取 1 个可用来读的 fd。
    /// “用户可接收”区域 -> “用户已接收”区域
    pub(crate) fn user_recv_one(&mut self) -> Option<&FrameDesc> {
        if self.user_can_recv_len == 0 {
            return None;
        }

        let rx_fd = &self.rx_fds[self.user_can_recv_pos];

        self.user_can_recv_len -= 1;
        self.user_has_recv_len += 1;
        self.user_can_recv_pos = advance(self.user_can_recv_pos, 1, self.rx_fds.len());

        Some(rx_fd)
    }

    pub(crate) fn get_fd_can_read(&mut self) -> Option<&FrameDesc> {
        // 当用户已读的fd足够多时，我们批量向内核提供
        // PERF: 回收的时机不宜太早（性能考量），也不宜太晚（内核没有可用的 fd 会导致丢数据）
        if self.user_has_recv_len() >= (FC / 2) {
            self.user_produce();
        }

        // 当用户没有可读的fd时，我们尝试从内核收回已经有数据的fd
        if self.user_can_recv_len() == 0 {
            self.user_consume();
        }

        self.user_recv_one()
    }
}

/// ```txt
/// |____kernel_has_send____|____user_has_write____|____user_can_write____
/// ```
#[derive(Debug)]
pub(crate) struct XdpWriter<const FC: usize> {
    pub(crate) tx_q: TxQueue,
    pub(crate) tx_fds: [FrameDesc; FC],
    // 内核处理完要发送的数据后，用户需要consume数据对应的fd
    pub(crate) cq: CompQueue,

    /// “用户可写”区的起始点。
    /// 用户可写意味着用户可用该 fd 来发送数据。
    user_can_write_pos: usize,

    /// “用户已写”区的起始点。
    user_has_write_pos: usize,

    /// “内核已发送”区的起始点。
    kernel_has_send_pos: usize,

    // 为了区分 empty 和 full，引入长度。
    //
    /// “用户已写” -> “用户可写”
    user_has_write_len: usize,

    /// “内核已发送” -> “用户已写”
    kernel_has_send_len: usize,
}

impl<const FC: usize> XdpWriter<FC> {
    /// “用户可写”区域的长度 (空闲空间，待用户写入)
    pub(crate) const fn user_can_write_len(&self) -> usize {
        self.tx_fds.len() - self.kernel_has_send_len - self.user_has_write_len
    }

    /// “用户已写”区域的长度 (已有数据，待内核发送)
    pub(crate) const fn user_has_write_len(&self) -> usize {
        self.user_has_write_len
    }

    /// “内核已发送”区域的长度 (已发送数据，待用户回收)
    pub(crate) const fn kernel_has_send_len(&self) -> usize {
        self.kernel_has_send_len
    }

    /// 用户完成写入，将 1 个描述符提交给内核。
    /// “用户可写”区域 -> “用户已写”区域
    pub(crate) fn user_write_one(&mut self) -> Option<&mut FrameDesc> {
        if self.user_can_write_len() == 0 {
            return None;
        }

        let tx_fd = &mut self.tx_fds[self.user_can_write_pos];

        self.user_has_write_len += 1;
        self.user_can_write_pos = advance(self.user_can_write_pos, 1, FC);

        Some(tx_fd)
    }

    /// 用户向内核提供已经写入数据的 fd，内核将它们发送。
    /// “用户已写”区域 -> “内核已发送”区域
    ///
    /// *考虑到性能和延迟，由用户自行决定何时调用以平衡两者。*
    pub(crate) fn user_produce_and_wakeup(&mut self) -> io::Result<usize> {
        let mut n_produce = 0;

        // 用户已写区
        let (s1, s2) = advance_get_mut(
            self.user_has_write_len(),
            self.user_has_write_pos,
            self.user_can_write_pos,
            &mut self.tx_fds,
        );

        if !s1.is_empty() {
            let n = unsafe { self.tx_q.produce(s1) };
            self.user_has_write_len -= n;
            self.kernel_has_send_len += n;
            self.user_has_write_pos = advance(self.user_has_write_pos, n, FC);

            n_produce += n;
            if n != s1.len() {
                if self.tx_q.needs_wakeup() {
                    self.tx_q.wakeup()?;
                }
                return Ok(n_produce);
            }
        }
        if !s2.is_empty() {
            let n = unsafe { self.tx_q.produce(s2) };
            self.user_has_write_len -= n;
            self.kernel_has_send_len += n;
            self.user_has_write_pos = advance(self.user_has_write_pos, n, FC);

            n_produce += n;
            if n != s2.len() {
                if self.tx_q.needs_wakeup() {
                    self.tx_q.wakeup()?;
                }
                return Ok(n_produce);
            }
        }

        debug_assert_eq!(
            self.user_has_write_len, 0,
            "user_has_write_len should be zero after producing all"
        );

        if self.tx_q.needs_wakeup() {
            self.tx_q.wakeup()?;
        }

        Ok(n_produce)
    }

    /// 用户尝试收回已经被内核发送的 fd。
    /// “内核已发送”区域 -> “用户可写”区域
    pub(crate) fn user_consume(&mut self) -> usize {
        let mut n_consume = 0;

        let (s1, s2) = advance_get_mut(
            self.kernel_has_send_len(),
            self.kernel_has_send_pos,
            self.user_has_write_pos,
            &mut self.tx_fds,
        );

        if !s1.is_empty() {
            let n = unsafe { self.cq.consume(s1) };
            self.kernel_has_send_len -= n;
            self.kernel_has_send_pos = advance(self.kernel_has_send_pos, n, FC);

            n_consume += n;
            if n != s1.len() {
                return n_consume;
            }
        }

        if !s2.is_empty() {
            let n = unsafe { self.cq.consume(s2) };
            self.kernel_has_send_len -= n;
            self.kernel_has_send_pos = advance(self.kernel_has_send_pos, n, FC);

            n_consume += n;
            if n != s2.len() {
                return n_consume;
            }
        }

        debug_assert_eq!(
            self.kernel_has_send_len, 0,
            "kernel_has_send_len should be zero after consuming all"
        );

        n_consume
    }

    pub(crate) fn get_fd_can_write(&mut self) -> Option<&mut FrameDesc> {
        // 当用户已写的fd足够多时，我们批量向内核提供，不再等待用户指令。
        if self.user_has_write_len() >= (FC / 2) {
            self.user_produce_and_wakeup().ok();
        }

        // 当用户没有可写的fd时，我们尝试从内核收回已经发送的fd
        if self.user_can_write_len() == 0 {
            self.user_consume();
        }

        self.user_write_one()
    }
}

impl<const FC: usize> AsRawFd for XdpDevice<FC> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}

impl<const FC: usize> Device for XdpDevice<FC> {
    type RxToken<'token>
        = XskRxToken<'token>
    where
        Self: 'token;
    type TxToken<'token>
        = XskTxToken<'token>
    where
        Self: 'token;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let rx_token = XskRxToken {
            umem: &self.umem,
            fd: self.reader.get_fd_can_read()?,
        };
        let tx_token = XskTxToken {
            umem: &self.umem,
            fd: self.writer.get_fd_can_write()?,
        };

        Some((rx_token, tx_token))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(XskTxToken {
            umem: &self.umem,
            fd: self.writer.get_fd_can_write()?,
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

/// 接收数据的流程：
///     1. 调用 FillQueue 的 produce() 方法，向内核提供 UMEM 中可用的 FrameDesc
///     2. 等待内核将数据写入 FrameDesc 指向的区域
///     3. 调用 RxQueue 的 consume() 方法，获取内核写入数据的 FrameDesc
pub(crate) struct XskRxToken<'a> {
    umem: &'a Umem,
    // 指向umem某个frame
    fd: &'a FrameDesc,
}

impl<'a> RxToken for XskRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let data = unsafe { self.umem.data(self.fd) };

        trace!("xdp recv: {:?}", data.contents());

        f(data.contents())
    }
}

/// When this token is consum, the kernel will not be awakened.
/// It is up to the user to decide when to wake it up (by calling ['wakeup_kernel']).
///
/// 发送数据的流程：
///     1. 从 UMEM 获取一个可用的 FrameDesc
///     2. 向 FrameDesc 指向的区域写入数据
///     3. 调用 TxQueue 的 produce_one() 方法，将 FrameDesc 交给内核处理
///     4. 用户选择在适当的时候调用 `wakeup_kernel()` 方法，唤醒内核处理发送队列
///     5. 此时可以调用 CompQueue 的 consume() 方法，获取内核处理完的数据对应的 FrameDesc
pub struct XskTxToken<'a> {
    umem: &'a Umem,
    // 指向umem某个frame
    fd: &'a mut FrameDesc,
}

impl<'a> TxToken for XskTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // 从 UMEM 获取整个帧的可变数据区域
        let mut data_mut = unsafe { self.umem.data_mut(self.fd) };
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

        result
    }
}

/// pos 在长度为 n_max的循环数组中前进 n 步。
const fn advance(pos: usize, n: usize, n_max: usize) -> usize {
    (pos + n) % n_max
}

/// 在循环数组中，获取从 pos1 开始，长度为 len 的区域，分割成两个切片返回。
fn advance_get_mut<T>(len: usize, pos1: usize, pos2: usize, arr: &mut [T]) -> (&mut [T], &mut [T]) {
    let n_max = arr.len();

    assert!(len <= n_max, "Length cannot exceed buffer capacity");
    assert!(
        len == 0 || len == n_max || (pos1 + len) % n_max == pos2,
        "pos1, pos2, and len are inconsistent"
    );

    // Case 1: 明确的“空”状态
    if len == 0 {
        return (&mut [], &mut []);
    }

    // Case 2: 明确的“满”状态
    if len == n_max {
        // 区域从 pos1 环绕一整圈。我们以 pos1 为界分割整个数组。
        let (left, right) = arr.split_at_mut(pos1);
        // 逻辑顺序是 right -> left
        return (right, left);
    }

    // Case 3: 部分填充状态 (0 < len < N), pos1 和 pos2 必然不相等
    if pos1 < pos2 {
        // 3a: 不环绕。这是一个单一的连续块。
        (&mut arr[pos1..pos2], &mut [])
    } else {
        // 3b: 环绕。
        let (left, right) = arr.split_at_mut(pos1);
        (right, &mut left[..pos2])
    }
}

#[cfg(test)]
#[serial_test::serial]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use smoltcp::{
        phy::{Device, RxToken, TxToken},
        time::Instant,
        wire::*,
    };
    use std::io::Write;
    use xsk_rs::config::XdpFlags;

    const FRAME_COUNT: usize = 16;

    fn create_device(if_name: &str) -> XdpDevice<FRAME_COUNT> {
        let sk_conf = SocketConfig::builder()
            .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE) // 使用 SKB 模式
            .build();

        XdpDevice::new_with_config(if_name, sk_conf).unwrap()
    }

    fn total_len(msg: &[u8]) -> usize {
        msg.len()
    }

    fn fill_send_buf(buf: &mut [u8], msg: &[u8]) {
        buf[..msg.len()].copy_from_slice(msg);
    }

    fn check_recv_buf(buf: &[u8], msg: &[u8]) {
        assert_eq!(buf, msg)
    }

    #[test]
    fn test_xsk_tx_token() {
        setup();

        let mut device1 = create_device(INTERFACE_NAME1);
        let msg = b"Hello XDP!";

        let tx_token = device1.transmit(Instant::now()).unwrap();

        let frame_len = EthernetFrame::<&[u8]>::header_len() + msg.len();

        tx_token.consume(frame_len, |buf| fill_send_buf(buf, msg));

        let data = unsafe { device1.umem.data(&device1.writer.tx_fds[0]) };
        assert_eq!(data.len(), frame_len);
    }

    #[test]
    fn test_xdp_writer() {
        setup();

        let mut device1 = create_device(INTERFACE_NAME1);
        let writer = &mut device1.writer;

        // WARN:
        // 发送的数据长度必须大于某个值，否则user_consume会出错，这个值与批量consume的量正相关，原因未知。
        // 例如发送了很多数据才调用一次 user_consume 则数据长度要求更大。
        let msg = [0_u8; 64];

        let n = FRAME_COUNT - 1;
        for i in 1..=n {
            let fd = writer.user_write_one().unwrap();
            let mut data_mut = unsafe { device1.umem.data_mut(fd) };
            data_mut.cursor().write_all(msg.as_slice()).unwrap();

            assert_eq!(writer.user_can_write_len(), FRAME_COUNT - i);
            assert_eq!(writer.user_has_write_len(), i);
            assert_eq!(writer.kernel_has_send_len(), 0);
        }

        let n_produce = writer.user_produce_and_wakeup().unwrap();
        assert_eq!(n_produce, n);

        assert_eq!(writer.user_can_write_len(), FRAME_COUNT - n);
        assert_eq!(writer.user_has_write_len(), 0);
        assert_eq!(writer.kernel_has_send_len(), n);

        let n_consume = writer.user_consume();
        assert_eq!(n_consume, n);

        assert_eq!(writer.user_can_write_len(), FRAME_COUNT);
        assert_eq!(writer.user_has_write_len(), 0);
        assert_eq!(writer.kernel_has_send_len(), 0);
    }

    #[test]
    fn test_xdp_reader() {
        setup();

        let mut device1 = create_device(INTERFACE_NAME1);
        let mut device2 = create_device(INTERFACE_NAME2);
        let writer = &mut device1.writer;
        let reader = &mut device2.reader;

        let n = FRAME_COUNT - 1;
        for i in 1..=n {
            let msg = [i as u8; 64];

            let fd = writer.user_write_one().unwrap();
            let mut data_mut = unsafe { device1.umem.data_mut(fd) };
            data_mut.cursor().write_all(&msg).unwrap();
        }

        let n_produce = writer.user_produce_and_wakeup().unwrap();
        assert_eq!(n_produce, n);
        let n_consume = writer.user_consume();
        assert_eq!(n_consume, n);

        let n_consume = reader.user_consume();
        assert_eq!(n_consume, n);

        for i in 1..=n {
            let fd = reader.user_recv_one().unwrap();
            let data = unsafe { device2.umem.data(fd) };
            assert_eq!(data.contents(), &[i as u8; 64]);

            assert_eq!(reader.kernel_can_write_len(), FRAME_COUNT - n);
            assert_eq!(reader.user_can_recv_len(), n - i);
            assert_eq!(reader.user_has_recv_len(), i);
        }
    }

    #[test]
    fn test_xdp_reader_and_writer() {
        setup();

        let mut device1 = create_device(INTERFACE_NAME1);
        let mut device2 = create_device(INTERFACE_NAME2);
        let writer = &mut device1.writer;
        let reader = &mut device2.reader;

        let n = FRAME_COUNT * 2;
        for i in 1..=n {
            let msg = [i as u8; 64];

            let fd = writer.get_fd_can_write().unwrap();
            let mut data_mut = unsafe { device1.umem.data_mut(fd) };
            data_mut.cursor().write_all(&msg).unwrap();

            writer.user_produce_and_wakeup().unwrap();
            writer.user_consume();

            let fd = reader.get_fd_can_read().unwrap();
            let data = unsafe { device2.umem.data(fd) };
            assert_eq!(data.contents(), &msg);
        }
    }

    #[test]
    fn test_device_send_and_recv() {
        setup();

        let mut device1 = create_device(INTERFACE_NAME1);
        let mut device2 = create_device(INTERFACE_NAME2);

        let n = FRAME_COUNT - 1;
        for i in 1..=n {
            let msg = [i as u8; 64];

            let tx_token = device1.transmit(Instant::now()).unwrap();
            tx_token.consume(total_len(&msg), |buf| fill_send_buf(buf, &msg));

            device1.flush().unwrap();

            let (rx_token, _) = device2.receive(Instant::now()).unwrap();
            rx_token.consume(|buf| check_recv_buf(buf, &msg))
        }
    }
}
