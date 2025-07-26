use eyre::{Result, bail};
use smoltcp::{
    iface::{Interface, SocketHandle, SocketSet},
    phy::{Checksum, Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::tcp::{self, Socket, SocketBuffer, State},
    time::Instant,
};
use std::{
    fmt::Debug,
    io::{self, Read, Write},
    net::{SocketAddr, ToSocketAddrs},
    num::NonZeroU32,
    thread::sleep,
    time::Duration,
};
use xsk_rs::{
    CompQueue, FillQueue, FrameDesc,
    config::{SocketConfig, UmemConfig, XdpFlags},
    socket::{RxQueue, Socket as XskSocket, TxQueue},
    umem::Umem,
};

use crate::config::AppConfig;

/// 我们让 AF_XDP 独占一张网卡(实际上是独占一个网卡队列)，因此 AfXdpStream 包含 iface 等字段
pub struct AfXdpStream {
    iface: Interface,
    device: XskDevice,
    sockets: SocketSet<'static>,
    handle: SocketHandle,
}

impl AfXdpStream {
    pub fn connect(addr: SocketAddr, mut iface: Interface, mut device: XskDevice) -> Result<Self> {
        let mut sockets = SocketSet::new(vec![]);
        let tcp_socket = Socket::new(
            SocketBuffer::new(vec![0; 4096]),
            SocketBuffer::new(vec![0; 4096]),
        );
        let handle = sockets.add(tcp_socket);

        let socket = sockets.get_mut::<Socket>(handle);
        socket.connect(iface.context(), addr, 0)?;

        loop {
            iface.poll(Instant::now(), &mut device, &mut sockets);
            let socket = sockets.get::<Socket>(handle);
            if socket.state() == State::Established {
                break Ok(Self {
                    iface,
                    device,
                    sockets,
                    handle,
                });
            }
            sleep(Duration::from_millis(50));
        }
    }
}

impl Debug for AfXdpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AfXdpStream")
            .field("device", &self.device)
            .field("sockets", &self.sockets)
            .field("handle", &self.handle)
            .finish()
    }
}

impl Read for AfXdpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            self.iface
                .poll(Instant::now(), &mut self.device, &mut self.sockets);
            let socket = self.sockets.get_mut::<tcp::Socket>(self.handle);
            if socket.can_recv() {
                return match socket.recv_slice(buf) {
                    Ok(n) => Ok(n),
                    Err(_) => Err(io::Error::other("smoltcp recv error")),
                };
            }
            if !socket.is_active() || socket.state() == State::CloseWait {
                return Ok(0); // EOF
            }
        }
    }
}

impl Write for AfXdpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            self.iface
                .poll(Instant::now(), &mut self.device, &mut self.sockets);
            let socket = self.sockets.get_mut::<tcp::Socket>(self.handle);
            if socket.can_send() {
                return match socket.send_slice(buf) {
                    Ok(n) => {
                        self.iface
                            .poll(Instant::now(), &mut self.device, &mut self.sockets);
                        Ok(n)
                    }
                    Err(_) => Err(io::Error::other("smoltcp send error")),
                };
            }
            if !socket.is_active() {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "socket closed"));
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct XskDevice {
    rx_q: RxQueue,
    tx_q: TxQueue,
    cq: CompQueue,
    fq: FillQueue,
    umem: Umem,
    rx_fd: FrameDesc,
    tx_fd: FrameDesc,
}

impl XskDevice {
    pub fn new(if_name: &str) -> Result<Self> {
        let if_name_parsed = if_name.parse().unwrap();

        // WARN: FRAME_COUNT 只要 2 就够了，不要在高吞吐场景使用。
        let (umem, descs) = Umem::new(UmemConfig::default(), NonZeroU32::new(2).unwrap(), true)?;

        let socket_conf = SocketConfig::builder()
            .xdp_flags(XdpFlags::XDP_FLAGS_SKB_MODE)
            .build();

        let (tx_q, rx_q, fq_and_cq) =
            unsafe { XskSocket::new(socket_conf, &umem, &if_name_parsed, 0) }?;

        let (mut fq, cq) = fq_and_cq.expect("UMEM is not shared, fq and cq should exist");

        // 填充 Fill Queue，让内核有可用的帧来接收数据
        unsafe { fq.produce(&descs) };

        Ok(Self {
            rx_q,
            tx_q,
            cq,
            fq,
            umem,
            rx_fd: descs[0],
            tx_fd: descs[1],
        })
    }
}

impl Device for XskDevice {
    type RxToken<'token>
        = XskRxToken<'token>
    where
        Self: 'token;
    type TxToken<'token>
        = XskTxToken<'token>
    where
        Self: 'token;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // 检查接收队列是否有数据
        if self.rx_q.poll(0).unwrap_or(false) {
            let rx_token = XskRxToken {
                rx_q: &mut self.rx_q,
                fq: &mut self.fq,
                umem: &self.umem,
                desc: self.rx_fd,
            };
            let tx_token = XskTxToken {
                tx_q: &mut self.tx_q,
                cq: &mut self.cq,
                umem: &self.umem,
                desc: self.tx_fd,
            };
            Some((rx_token, tx_token))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(XskTxToken {
            tx_q: &mut self.tx_q,
            cq: &mut self.cq,
            umem: &self.umem,
            desc: self.tx_fd,
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
    rx_q: &'a mut RxQueue,
    fq: &'a mut FillQueue,
    umem: &'a Umem,
    desc: FrameDesc,
}

impl<'a> RxToken for XskRxToken<'a> {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let data = unsafe {
            self.rx_q.consume_one(&mut self.desc);
            self.fq.produce_one(&self.desc);
            self.umem.data(&self.desc)
        };
        f(data.contents())
    }
}

pub struct XskTxToken<'a> {
    tx_q: &'a mut TxQueue,
    cq: &'a mut CompQueue,
    umem: &'a Umem,
    desc: FrameDesc,
}

impl<'a> TxToken for XskTxToken<'a> {
    fn consume<R, F>(mut self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut data = unsafe { self.umem.data_mut(&mut self.desc) };
        let mut buffer = data.contents_mut();
        // if len > buffer.len() {
        //     bail!("Buffer length {} exceeds available space {}", len, buffer.len());
        // }

        let result = f(&mut buffer);

        unsafe {
            self.tx_q.produce_one_and_wakeup(&self.desc).unwrap();
            // WARN: 此处立即消费 CQ 不符合 AF_XDP 异步模型，但对低吞吐场景有效。
            self.cq.consume_one(&mut self.desc);
        }

        result
    }
}
