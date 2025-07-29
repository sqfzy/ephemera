#!/usr/bin/env nu

# --- 配置常量 ---
const NS_NAME = "xdp_test_ns"
const VETH_HOST = "veth_host"
const VETH_GUEST = "veth_guest"
const SUBNET = "192.168.100.0/24"
const IP_HOST = "192.168.100.1/24"
const IP_GUEST = "192.168.100.2/24"
# 自动获取 WSL 的主网络接口 (通常是 eth0)
let MAIN_IF = (^ip -4 route get 1.1.1.1 | rg 'dev' | split row ' ' | get 4)

# === `setup` 命令: 创建环境 ===
def setup [] {
    print $"[*] Setting up isolated network with Internet access..."
    print $"[*] Main interface detected as: ($MAIN_IF)"

    # 检查 iptables 是否可用
    if (which iptables | is-empty) {
        print -e "[!] Error: `iptables` command not found. Please install it."
        return
    }

    # 步骤 1: 开启内核 IP 转发
    print "\n[1/6] Enabling kernel IP forwarding..."
    ^sudo sysctl -w net.ipv4.ip_forward=1

    # 步骤 2: 设置 iptables NAT 伪装规则
    print "\n[2/6] Setting up iptables NAT masquerade rules..."
    # -t nat: 操作NAT表, -A POSTROUTING: 在路由后修改包, -s: 源地址为我们的子网, -o: 出口为物理网卡
    ^sudo iptables -t nat -A POSTROUTING -s $SUBNET -o $MAIN_IF -j MASQUERADE
    # 允许流量在 veth_host 和主网卡之间转发
    ^sudo iptables -A FORWARD -i $VETH_HOST -o $MAIN_IF -j ACCEPT
    ^sudo iptables -A FORWARD -i $MAIN_IF -o $VETH_HOST -m state --state RELATED,ESTABLISHED -j ACCEPT


    # 步骤 3: 创建网络命名空间
    print $"\n[3/6] Creating network namespace: ($NS_NAME)"
    ^sudo ip netns add $NS_NAME

    # 步骤 4: 创建 veth 设备对
    print $"\n[4/6] Creating veth pair: ($VETH_HOST) <--> ($VETH_GUEST)"
    ^sudo ip link add $VETH_HOST type veth peer name $VETH_GUEST

    # 步骤 5: 将 veth_guest 移动到沙箱
    print $"\n[5/6] Moving '($VETH_GUEST)' into namespace '($NS_NAME)'"
    ^sudo ip link set $VETH_GUEST netns $NS_NAME

    # 步骤 6: 配置网络接口和路由
    print "\n[6/6] Configuring IP addresses, interfaces, and routes..."
    # 6a: 配置根命名空间中的 veth_host
    ^sudo ip addr add $IP_HOST dev $VETH_HOST
    ^sudo ip link set $VETH_HOST up
    print $"    - Host side '($VETH_HOST)' is UP"

    # 6b: 配置沙箱中的 veth_guest, lo, 以及默认路由
    let guest_ip = ($IP_GUEST | str replace '/24' '')
    let gateway_ip = ($IP_HOST | str replace '/24' '')
    ^sudo ip netns exec $NS_NAME ip addr add $IP_GUEST dev $VETH_GUEST
    ^sudo ip netns exec $NS_NAME ip link set $VETH_GUEST up
    ^sudo ip netns exec $NS_NAME ip link set lo up
    ^sudo ip netns exec $NS_NAME ip route add default via $gateway_ip
    print $"    - Guest side '($VETH_GUEST)' is UP with gateway ($gateway_ip)"


    # --- 完成 ---
    print $"\n[+] Success! Environment is ready with Internet access."
    print $"\n========================= VERIFY & USE ========================="
    print $"1.  Verify internet connectivity from the namespace:"
    print $"    > sudo ip netns exec ($NS_NAME) ping -c 3 8.8.8.8"
    print $"\n2.  Get the MAC address of '($VETH_GUEST)' for your config:"
    print $"    > sudo ip netns exec ($NS_NAME) cat /sys/class/net/($VETH_GUEST)/address"
    print $"\n3.  Configure 'config.toml' (cat config.toml)."
    print $"\n4.  When finished, clean up with:"
    print $"    > nu ($env.CURRENT_FILE) teardown"
    print $"=================================================================="
}


# === `teardown` 命令: 清理环境 ===
def teardown [] {
    print $"[*] Tearing down network environment..."

    # 检查命名空间是否存在
    if ( ^ip netns list | find $NS_NAME | is-empty ) {
        print $"\n[+] Namespace '($NS_NAME)' does not exist. Cleaning up iptables rules just in case..."
    } else {
      # 删除网络命名空间会一并删除 veth 对
      ^sudo ip netns del $NS_NAME
      print "[+] Namespace and veth pair removed."
    }

    # 清理 iptables 规则 (-D for delete)
    print "[+] Cleaning up iptables rules..."
    ^sudo iptables -t nat -D POSTROUTING -s $SUBNET -o $MAIN_IF -j MASQUERADE
    ^sudo iptables -D FORWARD -i $VETH_HOST -o $MAIN_IF -j ACCEPT
    ^sudo iptables -D FORWARD -i $MAIN_IF -o $VETH_HOST -m state --state RELATED,ESTABLISHED -j ACCEPT

    # (可选) 恢复 IP 转发设置
    # print "[+] Restoring IP forwarding setting."
    # ^sudo sysctl -w net.ipv4.ip_forward=0

    print $"\n[+] Teardown complete."
}


# === 主入口点 ===
export def main [
    mode: string # The command to run: "setup" or "teardown"
] {
    match $mode {
        "setup" => { setup }
        "teardown" => { teardown }
        _ => {
            print -e "[!] Error: Invalid command '($mode)'."
            print $"\nUsage: nu ($env.CURRENT_FILE) <command>"
            print   "Available commands: setup, teardown"
        }
    }
}
