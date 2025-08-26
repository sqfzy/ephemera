要让Reactor尽力poll吗？这样的话调用者使用poll时，可能无法看见socket的每个状态变化

smoltcp是面向状态的，而不是过程

将Reactor架构转为epoll多路复用


bpf程序没问题，可以正常转发。但程序有时候能收到，有时候收不到。
可以收到ARP包，但为什么不能收到TCP包？

不要`sudo -E ~/.cargo/bin/cargo build`，否则link时会报错

把xdp代码单独成库

动态allow ip? 但listener无法在不allow remote ip的情况下取得allow remote ip

xdp 使用 tokio_websockets，出现"operation would block" error. 代码：

```
Connector::new()?.wrap(host, stream).await?
```

`wrap()`返回该错误。tokio_native_tls::TlsConnector::connect()返回该错误。
### 解决
将`poll_flush`改为:

```
while socket.send_queue() != 0 {
    reactor.poll_and_flush()?;
    println!("debug0: poll_flush ok");
    return Poll::Ready(Ok(()));
}
```

即不要只`poll_and_flush`一次，这样做不会发送send buffer的所有数据


会卡住，可能是注册后没有wakeup

xdp okx有时候会卡在sending SYN，发现没有成功添加到白名单

提供setup_xdp

# 修改了libbpf-sys
build.rs
`# WERROR := -Werror`

可以试试libxdp提供的程序
