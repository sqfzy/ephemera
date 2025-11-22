use smoltcp::{
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    time::Instant,
};
use std::{
    fmt::Debug,
    io,
    num::NonZeroU32,
    os::fd::{AsRawFd, RawFd},
};
use tracing::trace;
use xsk_rs::{
    CompQueue, FillQueue, FrameDesc,
    config::{BindFlags, LibxdpFlags, SocketConfig, UmemConfig, XdpFlags},
    socket::{RxQueue, Socket as XskSocket, TxQueue},
    umem::Umem,
};

// pub(crate) fn get_default_if_name() -> io::Result<String> {
//     netdev::get_default_interface().map_or_else(
//         |e| {
//             Err(io::Error::new(
//                 io::ErrorKind::NotFound,
//                 format!("Failed to get default interface: {}", e),
//             ))
//         },
//         |interface| interface.name,
//     )
// }

#[derive(Debug, Clone, bon::Builder)]
pub struct XdpDeviceConfig<const FC: usize = 1024> {
    /// Network interface name (e.g., "eth0")
    /// Default behavior: use the system's default network interface name or empty string if not found
    #[builder(into, default = netdev::get_default_interface().map(|i|i.name).unwrap_or_default())]
    pub if_name: String,

    /// Network card queue ID
    #[builder(default = 0)]
    pub queue_id: u32,

    /// Rx batch threshold
    #[builder(default = 512)]
    pub rx_batch_threshold: usize,

    /// Tx batch threshold (defaults to half of the frame count)
    #[builder(default = FC / 2)]
    pub tx_batch_threshold: usize,

    /// Whether to use Huge Pages
    #[builder(default = false)]
    pub use_huge_pages: bool,

    /// Libxdp specific flags
    pub libxdp_flags: Option<LibxdpFlags>,

    /// XDP flags (e.g., SKB_MODE vs DRV_MODE)
    #[builder(default = XdpFlags::XDP_FLAGS_SKB_MODE)]
    pub xdp_flags: XdpFlags,

    /// Basic bind flags
    #[builder(default = BindFlags::XDP_USE_NEED_WAKEUP)]
    pub bind_flags: BindFlags,
}

impl<const FC: usize> TryFrom<XdpDeviceConfig<FC>> for XdpDevice<FC> {
    type Error = io::Error;

    fn try_from(config: XdpDeviceConfig<FC>) -> Result<Self, Self::Error> {
        XdpDevice::new(config)
    }
}

#[derive(Debug)]
pub struct XdpDevice<const FC: usize = 1024> {
    reader: XdpReader<FC>,
    writer: XdpWriter<FC>,
    umem: Umem,
    fd: RawFd,
    config: XdpDeviceConfig<FC>,
}

impl<const FC: usize> XdpDevice<FC> {
    pub fn new(config: XdpDeviceConfig<FC>) -> io::Result<Self> {
        let XdpDeviceConfig {
            if_name,
            queue_id,
            rx_batch_threshold,
            tx_batch_threshold,
            use_huge_pages,
            libxdp_flags,
            xdp_flags,
            bind_flags,
        } = config.clone();

        // 1. Parse interface name (xsk_rs requires a specific Interface type)
        let if_name_parsed = if_name.parse().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Failed to parse interface name: {}", e),
            )
        })?;

        // 2. Calculate total frame count (Rx + Tx)
        let total_frame_count = NonZeroU32::new((FC * 2) as u32).ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Frame count must be greater than zero",
        ))?;

        // 3. Create Umem (User space memory area)
        let (umem, descs) = Umem::new(UmemConfig::default(), total_frame_count, use_huge_pages)
            .map_err(io::Error::other)?;

        // 4. Split frame descriptors (Rx first half, Tx second half)
        let rx_fds: [FrameDesc; FC] = descs[..FC]
            .try_into()
            .expect("Failed to split frame descriptors for Rx");
        let tx_fds: [FrameDesc; FC] = descs[FC..]
            .try_into()
            .expect("Failed to split frame descriptors for Tx");

        // 5. Configure Socket
        let mut socket_config_builder = SocketConfig::builder();

        if let Some(flags) = libxdp_flags {
            socket_config_builder.libxdp_flags(flags);
        }

        let socket_config = socket_config_builder
            .xdp_flags(xdp_flags)
            .bind_flags(bind_flags)
            .build();

        // 6. Create AF_XDP Socket
        let (tx_q, rx_q, fq_and_cq) =
            unsafe { XskSocket::new(socket_config, &umem, &if_name_parsed, queue_id) }
                .map_err(io::Error::other)?;

        // fq (Fill Queue) and cq (Completion Queue) must exist because we are not sharing umem
        let (mut fq, cq) = fq_and_cq.expect("Unsupport shared umem. Try another queue id");

        // 7. Initialize Fill Queue
        // Must first put Rx frame descriptors into Fill Queue, telling the kernel these frames can be used to receive data
        unsafe {
            fq.produce(&rx_fds);
        }

        let fd = tx_q.fd().as_raw_fd();

        let reader = XdpReader::new(rx_q, rx_fds, fq, rx_batch_threshold);
        let writer = XdpWriter::new(tx_q, tx_fds, cq, tx_batch_threshold);

        Ok(Self {
            reader,
            writer,
            umem,
            fd,
            config,
        })
    }

    pub fn config(&self) -> &XdpDeviceConfig<FC> {
        &self.config
    }

    /// Flush the transmit queue, submitting all pending data to the kernel
    pub(crate) fn flush(&mut self) -> io::Result<usize> {
        self.writer.user_produce_and_wakeup()
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
        // TODO: Make configurable
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 3000;
        caps.medium = Medium::Ethernet;
        // caps.checksum.ipv4 = Checksum::Tx;
        // caps.checksum.tcp = Checksum::Tx;
        caps
    }
}

/// XDP Receive Queue Manager
///
/// # Three-stage buffer layout:
/// ```text
/// |____user_has_recv____|____user_can_recv____|____kernel_can_write____
///       Read (User)           Pending Read         Writable (Kernel)
/// ```
///
/// # Invariants:
/// - `user_has_recv_len + user_can_recv_len + kernel_can_write_len() == FC`
/// - Regions do not overlap
/// - All position indices < FC
#[derive(Debug)]
pub(crate) struct XdpReader<const FC: usize> {
    pub(crate) rx_q: RxQueue,
    pub(crate) rx_fds: [FrameDesc; FC],
    pub(crate) fq: FillQueue,

    kernel_can_write_pos: usize,
    user_can_recv_pos: usize,
    user_has_recv_pos: usize,

    user_can_recv_len: usize,
    user_has_recv_len: usize,

    /// Batch threshold: When read frames exceed this value, batch return to kernel
    rx_batch_threshold: usize,
}

impl<const FC: usize> XdpReader<FC> {
    pub(crate) fn new(
        rx_q: RxQueue,
        rx_fds: [FrameDesc; FC],
        fq: FillQueue,
        rx_batch_threshold: usize,
    ) -> Self {
        Self {
            rx_q,
            rx_fds,
            fq,
            kernel_can_write_pos: 0,
            user_can_recv_pos: 0,
            user_has_recv_pos: 0,
            user_can_recv_len: 0,
            user_has_recv_len: 0,
            rx_batch_threshold,
        }
    }

    /// Length of "Kernel can Write" area (free space, waiting for kernel to write)
    #[inline]
    pub(crate) const fn kernel_can_write_len(&self) -> usize {
        FC - self.user_can_recv_len - self.user_has_recv_len
    }

    /// Length of "User can Read" area (has data, waiting for user to read)
    #[inline]
    pub(crate) const fn user_can_recv_len(&self) -> usize {
        self.user_can_recv_len
    }

    /// Length of "User has Recv" area (read, waiting to return to kernel)
    #[inline]
    pub(crate) const fn user_has_recv_len(&self) -> usize {
        self.user_has_recv_len
    }

    /// User returns read frame descriptors to the kernel
    ///
    /// "User has Recv" area -> "Kernel can Write" area
    ///
    /// # Return
    /// Number of frames successfully produced/returned
    pub(crate) fn user_produce(&mut self) -> usize {
        let mut n_produce = 0;

        // User has received area
        let (s1, s2) = advance_get_mut(
            self.user_has_recv_len(),
            self.user_has_recv_pos,
            self.user_can_recv_pos,
            &mut self.rx_fds,
        );

        if !s1.is_empty() {
            // SAFETY: Frames in s1 have been read by the user, now returning to kernel
            let n = unsafe { self.fq.produce(s1) };
            self.user_has_recv_len -= n;
            self.user_has_recv_pos = advance(self.user_has_recv_pos, n, FC);
            n_produce += n;

            if n != s1.len() {
                return n_produce;
            }
        }

        if !s2.is_empty() {
            // SAFETY: Frames in s2 have been read by the user, now returning to kernel
            let n = unsafe { self.fq.produce(s2) };
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

    /// User acquires frames filled with data from the kernel
    ///
    /// "Kernel can Write" area -> "User can Recv" area
    ///
    /// # Return
    /// Number of frames successfully consumed/acquired
    pub(crate) fn user_consume(&mut self) -> usize {
        let mut n_consume = 0;

        let (s1, s2) = advance_get_mut(
            self.kernel_can_write_len(),
            self.kernel_can_write_pos,
            self.user_has_recv_pos,
            &mut self.rx_fds,
        );

        if !s1.is_empty() {
            // SAFETY: Frames in s1 currently belong to kernel, we try to acquire frames filled by kernel
            let n = unsafe { self.rx_q.consume(s1) };
            self.user_can_recv_len += n;
            self.kernel_can_write_pos = advance(self.kernel_can_write_pos, n, FC);
            n_consume += n;

            if n != s1.len() {
                return n_consume;
            }
        }

        if !s2.is_empty() {
            // SAFETY: Frames in s2 currently belong to kernel, we try to acquire frames filled by kernel
            let n = unsafe { self.rx_q.consume(s2) };
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

    /// User reads one available frame
    ///
    /// "User can Recv" area -> "User has Recv" area
    ///
    /// # Return
    /// Reference to readable frame descriptor, or None if no frames available
    pub(crate) fn user_recv_one(&mut self) -> Option<&FrameDesc> {
        if self.user_can_recv_len == 0 {
            return None;
        }

        let rx_fd = &self.rx_fds[self.user_can_recv_pos];

        self.user_can_recv_len -= 1;
        self.user_has_recv_len += 1;
        self.user_can_recv_pos = advance(self.user_can_recv_pos, 1, FC);

        Some(rx_fd)
    }

    pub(crate) fn get_fd_can_read(&mut self) -> Option<&FrameDesc> {
        // When user has read enough fds, we return them to kernel in batch, not waiting for user instruction.
        if self.user_has_recv_len() >= self.rx_batch_threshold {
            self.user_produce();
        }

        // When user has no readable fds, we try to reclaim fds with data from kernel
        if self.user_can_recv_len() == 0 {
            self.user_consume();
        }

        self.user_recv_one()
    }
}

/// XDP Transmit Queue Manager
///
/// # Three-stage buffer layout:
/// ```text
/// |____kernel_has_send____|____user_has_write____|____user_can_write____
///       Sent (Kernel)          Written (User)         Writable (User)
/// ```
///
/// # Invariants:
/// - `kernel_has_send_len + user_has_write_len + user_can_write_len() == FC`
/// - Regions do not overlap
/// - All position indices < FC
#[derive(Debug)]
pub(crate) struct XdpWriter<const FC: usize> {
    pub(crate) tx_q: TxQueue,
    pub(crate) tx_fds: [FrameDesc; FC],
    pub(crate) cq: CompQueue,

    user_can_write_pos: usize,
    user_has_write_pos: usize,
    kernel_has_send_pos: usize,

    user_has_write_len: usize,
    kernel_has_send_len: usize,

    /// Batch threshold: Automatically submit to kernel when written frames exceed this value
    tx_batch_threshold: usize,
}

impl<const FC: usize> XdpWriter<FC> {
    pub(crate) fn new(
        tx_q: TxQueue,
        tx_fds: [FrameDesc; FC],
        cq: CompQueue,
        tx_batch_threshold: usize,
    ) -> Self {
        Self {
            tx_q,
            tx_fds,
            cq,
            user_can_write_pos: 0,
            user_has_write_pos: 0,
            kernel_has_send_pos: 0,
            user_has_write_len: 0,
            kernel_has_send_len: 0,
            tx_batch_threshold,
        }
    }

    /// Length of "User can Write" area (free space, waiting for user to write)
    #[inline]
    pub(crate) const fn user_can_write_len(&self) -> usize {
        FC - self.kernel_has_send_len - self.user_has_write_len
    }

    /// Length of "User has Write" area (data written, waiting to submit to kernel)
    #[inline]
    pub(crate) const fn user_has_write_len(&self) -> usize {
        self.user_has_write_len
    }

    /// Length of "Kernel has Send" area (sent, waiting to be reclaimed)
    #[inline]
    pub(crate) const fn kernel_has_send_len(&self) -> usize {
        self.kernel_has_send_len
    }

    /// User acquires a writable frame
    ///
    /// "User can Write" area -> "User has Write" area
    ///
    /// # Return
    /// Mutable reference to writable frame descriptor, or None if no frames available
    pub(crate) fn user_write_one(&mut self) -> Option<&mut FrameDesc> {
        if self.user_can_write_len() == 0 {
            return None;
        }

        let tx_fd = &mut self.tx_fds[self.user_can_write_pos];

        self.user_has_write_len += 1;
        self.user_can_write_pos = advance(self.user_can_write_pos, 1, FC);

        Some(tx_fd)
    }

    /// User submits written frames to kernel and wakes up kernel if necessary
    ///
    /// "User has Write" area -> "Kernel has Send" area
    ///
    /// # Return
    /// Number of frames successfully submitted
    pub(crate) fn user_produce_and_wakeup(&mut self) -> io::Result<usize> {
        let mut n_produce = 0;

        let (s1, s2) = advance_get_mut(
            self.user_has_write_len(),
            self.user_has_write_pos,
            self.user_can_write_pos,
            &mut self.tx_fds,
        );

        if !s1.is_empty() {
            // SAFETY: Frames in s1 have been written by user, now submitting to kernel for transmission
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
            // SAFETY: Frames in s2 have been written by user, now submitting to kernel for transmission
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

    /// User reclaims sent frames from the kernel
    ///
    /// "Kernel has Send" area -> "User can Write" area
    ///
    /// # Return
    /// Number of frames successfully reclaimed
    pub(crate) fn user_consume(&mut self) -> usize {
        let mut n_consume = 0;

        let (s1, s2) = advance_get_mut(
            self.kernel_has_send_len(),
            self.kernel_has_send_pos,
            self.user_has_write_pos,
            &mut self.tx_fds,
        );

        if !s1.is_empty() {
            // SAFETY: Frames in s1 currently belong to kernel, we try to reclaim sent frames
            let n = unsafe { self.cq.consume(s1) };
            self.kernel_has_send_len -= n;
            self.kernel_has_send_pos = advance(self.kernel_has_send_pos, n, FC);

            n_consume += n;
            if n != s1.len() {
                return n_consume;
            }
        }

        if !s2.is_empty() {
            // SAFETY: Frames in s2 currently belong to kernel, we try to reclaim sent frames
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

    /// Get a writable frame
    ///
    /// This method automatically handles:
    /// 1. Automatically submit to kernel when enough frames are written
    /// 2. Try to reclaim sent frames when no writable frames are available
    ///
    /// # Return
    /// Mutable reference to writable frame descriptor, or None if no frames available
    pub(crate) fn get_fd_can_write(&mut self) -> Option<&mut FrameDesc> {
        // When user has written enough fds, we batch submit to kernel, not waiting for user instruction.
        if self.user_has_write_len() >= self.tx_batch_threshold {
            self.user_produce_and_wakeup().ok();
        }

        // When user has no writable fds, we try to reclaim sent fds from kernel
        if self.user_can_write_len() == 0 {
            self.user_consume();
        }

        self.user_write_one()
    }
}

/// Receive Token
///
/// # Data Flow:
/// 1. User calls `FillQueue::produce()` to provide empty frames to kernel
/// 2. Kernel writes received packet data into frames
/// 3. User calls `RxQueue::consume()` to acquire filled frames
/// 4. Read data via this Token
///
/// # Safety
/// - During the lifetime of this Token, the corresponding frame belongs to user
/// - After `consume()`, frame ownership returns to queue manager
pub struct XskRxToken<'a> {
    umem: &'a Umem,
    // Points to a frame in umem
    fd: &'a FrameDesc,
}

impl<'a> RxToken for XskRxToken<'a> {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        // SAFETY: Memory area pointed to by fd has been written by kernel, now exclusive access by us
        let data = unsafe { self.umem.data(self.fd) };

        trace!("xdp recv: {:?}", data.contents());

        f(data.contents())
    }
}

/// Transmit Token
///
/// # Data Flow:
/// 1. Get free frame from UMEM
/// 2. Write data to frame via this Token
/// 3. Call `TxQueue::produce()` to submit to kernel
/// 4. After kernel transmission, reclaim via `CompQueue::consume()`
///
/// # Safety
/// - During the lifetime of this Token, the corresponding frame belongs to user
/// - After `consume()`, frame data will be read and sent by kernel
///
/// # Note
/// Consuming this Token does not immediately wake up the kernel.
/// The queue manager decides when to wake up to optimize batching performance.
pub struct XskTxToken<'a> {
    umem: &'a Umem,
    // Points to a frame in umem
    fd: &'a mut FrameDesc,
}

impl<'a> TxToken for XskTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        // SAFETY: Memory area pointed to by fd currently exclusively accessed by us
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

/// Advance position in circular array
///
/// # Arguments
/// - `pos`: Current position
/// - `n`: Steps to advance
/// - `n_max`: Array capacity
///
/// # Return
/// New position (wrapped around)
const fn advance(pos: usize, n: usize, n_max: usize) -> usize {
    (pos + n) % n_max
}

/// Get mutable slices of a specified region in a circular array
///
/// # Arguments
/// - `len`: Region length
/// - `pos1`: Region start position
/// - `pos2`: Region end position (exclusive)
/// - `arr`: Circular array
///
/// # Return
/// `(slice1, slice2)` - If region crosses array boundary, returns two slices; otherwise second slice is empty
///
/// # Panics
/// - If `len > arr.len()`
/// - If position arguments are inconsistent
fn advance_get_mut<T>(len: usize, pos1: usize, pos2: usize, arr: &mut [T]) -> (&mut [T], &mut [T]) {
    let n_max = arr.len();

    assert!(
        len <= n_max,
        "Length {} cannot exceed buffer capacity {}",
        len,
        n_max
    );
    assert!(
        len == 0 || len == n_max || (pos1 + len) % n_max == pos2,
        "Inconsistent positions: pos1={}, pos2={}, len={}, capacity={}",
        pos1,
        pos2,
        len,
        n_max
    );

    // Case 1: Empty region
    if len == 0 {
        return (&mut [], &mut []);
    }

    // Case 2: Full region (entire array)
    if len == n_max {
        // Region wraps around from pos1. We split the array at pos1.
        let (left, right) = arr.split_at_mut(pos1);
        // Logical order is right -> left
        return (right, left);
    }

    // Case 3: Partial region
    if pos1 < pos2 {
        // Does not cross boundary
        (&mut arr[pos1..pos2], &mut [])
    } else {
        // Crosses boundary
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
        XdpDeviceConfig::builder()
            .if_name(if_name)
            .queue_id(0)
            .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE)
            .build()
            .try_into()
            .unwrap()
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

        // WARN: The size of msg must be greater than the minimum Ethernet frame size (14 bytes)
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
