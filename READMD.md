`ephemera-data`: 核心数据结构模块。定义了项目中使用的各种数据类型和以及stream的transform操作。

`ephemera-source`: 数据源处理模块。该模块负责连接到外部数据源（例如交易所的 WebSocket 服务），并对接收到的原始数据进行解析和处理。

`ephemera-xdp`: XDP 功能实现模块。使用`smoltcp`封装了异步的`XdpTcpStream`和`XdpTcpListener`，并保持API与std一致（确保易用）。
