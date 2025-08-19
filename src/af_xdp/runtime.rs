use crate::{
    af_xdp::{device::XdpDevice, reactor::XdpReactor},
    config::EphemaraConfig,
};
use futures::{FutureExt, future::poll_fn, pin_mut, task::noop_waker_ref};
use smoltcp::{
    iface::{Interface, SocketSet},
    time::{Duration, Instant},
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};
use std::{
    collections::VecDeque,
    io,
    os::fd::AsRawFd,
    pin::Pin,
    sync::{LazyLock, Mutex},
    task::{Context, Poll},
};

/// ```
/// runtime.block_on(|reactor| async move {
///     r.spawn(|reactor| async {
///         do_something().await;
///     });
///
///     do_something().await;
/// });
///
/// ```
pub struct Runtime {
    pub reactor: XdpReactor,
    // tasks总是包含所有task，只有当task完成后，才会被移除
    tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Runtime {
    pub fn new(device: XdpDevice, mac_addr: impl Into<HardwareAddress>) -> Self {
        Self {
            reactor: XdpReactor::new().unwrap(),
            tasks: vec![],
        }
    }

    pub fn block_on<F, R>(&mut self, mut f: F) -> R
    where
        F: FnOnce(&mut XdpReactor) -> Pin<Box<dyn Future<Output = R>>>,
    {
        let mut cx = Context::from_waker(noop_waker_ref());

        let mut main_future = f(&mut self.reactor);
        loop {
            {
                if let Poll::Ready(result) = main_future.as_mut().poll(&mut cx) {
                    break result;
                }
            }

            self.tasks.retain_mut(|task| {
                match task.as_mut().poll(&mut cx) {
                    std::task::Poll::Ready(()) => false, // 移除已完成的任务
                    std::task::Poll::Pending => true,
                }
            });

            // 等待事件
            let now = Instant::now();
            let delay = self.reactor.iface.poll_delay(now, &self.reactor.sockets);
            smoltcp::phy::wait(self.reactor.device.as_raw_fd(), delay).ok();
        }
    }

    pub fn spawn<F>(&mut self, task: F)
    where
        F: FnOnce(&mut XdpReactor) -> Pin<Box<dyn Future<Output = ()>>>,
    {
        self.tasks.push(task(&mut self.reactor));
    }
}
