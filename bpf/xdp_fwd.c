// xdp_fwd.c
#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/in.h>
#include <linux/ip.h>
#include <linux/tcp.h>

#include <bpf/bpf_endian.h>
#include <bpf/bpf_helpers.h>

// AF_XDP 需要一个特殊的 map 类型：BPF_MAP_TYPE_XSKMAP
// 它将一个队列 ID (key) 映射到一个 AF_XDP socket (value)
struct {
  __uint(type, BPF_MAP_TYPE_XSKMAP);
  __uint(key_size, sizeof(int));
  __uint(value_size, sizeof(int));
  __uint(max_entries, 64); // 支持最多 64 个队列
} xsks_map SEC(".maps");

SEC("xdp")
int xdp_forwarder(struct xdp_md *ctx) {
  void *data_end = (void *)(long)ctx->data_end;
  void *data = (void *)(long)ctx->data;

  // 1. 解析以太网头
  struct ethhdr *eth = data;
  if ((void *)(eth + 1) > data_end) {
    return XDP_PASS; // 包太小，直接放行
  }

  // 2. 只处理 IPv4 包
  if (eth->h_proto != bpf_htons(ETH_P_IP)) {
    return XDP_PASS;
  }

  // 3. 解析 IP 头
  struct iphdr *iph = data + sizeof(*eth);
  if ((void *)(iph + 1) > data_end) {
    return XDP_PASS;
  }

  // 4. 只处理 TCP 包
  if (iph->protocol != IPPROTO_TCP) {
    return XDP_PASS;
  }

  // 5. 解析 TCP 头
  struct tcphdr *tcph = (void *)iph + sizeof(*iph);
  if ((void *)(tcph + 1) > data_end) {
    return XDP_PASS;
  }

  // 6. 检查目标端口是否为 8080
  if (tcph->dest == bpf_htons(8080)) {
    // 关键步骤：重定向到 xsk map
    // 使用接收队列的索引作为 map 的 key
    // 这是为了支持多队列网卡(RSS)
    return bpf_redirect_map(&xsks_map, ctx->rx_queue_index, 0);
  }

  // 对于其他所有包，让内核网络协议栈正常处理
  return XDP_PASS;
}

// BPF 程序必须有一个许可证
char LICENSE[] SEC("license") = "GPL";
