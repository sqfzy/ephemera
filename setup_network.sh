#!/bin/bash

# --- Configuration constants ---
NS_NAME="xdp_test_ns"
VETH_HOST="veth_host"
VETH_GUEST="veth_guest"
SUBNET="192.168.100.0/24"
IP_HOST="192.168.100.1/24"
IP_GUEST="192.168.100.2/24"

# Auto-detect main network interface
MAIN_IF=$(ip -4 route get 1.1.1.1 | grep -oP 'dev \K\S+' | head -1)

# === Setup command: Create environment ===
setup() {
    echo "[*] Setting up isolated network with Internet access..."
    echo "[*] Main interface detected as: $MAIN_IF"

    # Check if iptables is available
    if ! command -v iptables &> /dev/null; then
        echo "[!] Error: iptables command not found. Please install it."
        return 1
    fi

    # Step 1: Enable kernel IP forwarding
    echo -e "\n[1/6] Enabling kernel IP forwarding..."
    sudo sysctl -w net.ipv4.ip_forward=1

    # Step 2: Set up iptables NAT masquerade rules
    echo -e "\n[2/6] Setting up iptables NAT masquerade rules..."
    sudo iptables -t nat -A POSTROUTING -s $SUBNET -o $MAIN_IF -j MASQUERADE
    sudo iptables -A FORWARD -i $VETH_HOST -o $MAIN_IF -j ACCEPT
    sudo iptables -A FORWARD -i $MAIN_IF -o $VETH_HOST -m state --state RELATED,ESTABLISHED -j ACCEPT

    # Step 3: Create network namespace
    echo -e "\n[3/6] Creating network namespace: $NS_NAME"
    sudo ip netns add $NS_NAME

    # Step 4: Create veth device pair
    echo -e "\n[4/6] Creating veth pair: $VETH_HOST <--> $VETH_GUEST"
    sudo ip link add $VETH_HOST type veth peer name $VETH_GUEST

    # Step 5: Move veth_guest to sandbox
    echo -e "\n[5/6] Moving '$VETH_GUEST' into namespace '$NS_NAME'"
    sudo ip link set $VETH_GUEST netns $NS_NAME

    # Step 6: Configure network interfaces and routes
    echo -e "\n[6/6] Configuring IP addresses, interfaces, and routes..."
    
    # 6a: Configure veth_host in root namespace
    sudo ip addr add $IP_HOST dev $VETH_HOST
    sudo ip link set $VETH_HOST up
    echo "    - Host side '$VETH_HOST' is UP"

    # 6b: Configure veth_guest in sandbox, lo, and default route
    local guest_ip="${IP_GUEST%/*}"
    local gateway_ip="${IP_HOST%/*}"
    
    sudo ip netns exec $NS_NAME ip addr add $IP_GUEST dev $VETH_GUEST
    sudo ip netns exec $NS_NAME ip link set $VETH_GUEST up
    sudo ip netns exec $NS_NAME ip link set lo up
    sudo ip netns exec $NS_NAME ip route add default via $gateway_ip
    echo "    - Guest side '$VETH_GUEST' is UP with gateway $gateway_ip"

    # --- Completion ---
    echo -e "\n[+] Success! Environment is ready with Internet access."
    echo -e "\n========================= VERIFY & USE ========================="
    echo "1.  Verify internet connectivity from the namespace:"
    echo "    > sudo ip netns exec $NS_NAME ping -c 3 8.8.8.8"
    echo -e "\n2.  Get the MAC address of '$VETH_GUEST' for your config:"
    echo "    > sudo ip netns exec $NS_NAME cat /sys/class/net/$VETH_GUEST/address"
    echo -e "\n3.  Configure 'config.toml' (cat config.toml)."
    echo -e "\n4.  When finished, clean up with:"
    echo "    > $0 teardown"
    echo "=================================================================="
}

# === Teardown command: Clean up environment ===
teardown() {
    echo "[*] Tearing down network environment..."

    # Check if namespace exists
    if ! ip netns list | grep -q "$NS_NAME"; then
        echo -e "\n[+] Namespace '$NS_NAME' does not exist. Cleaning up iptables rules just in case..."
    else
        # Delete network namespace will also delete veth pair
        sudo ip netns del $NS_NAME
        echo "[+] Namespace and veth pair removed."
    fi

    # Clean up iptables rules (-D for delete)
    echo "[+] Cleaning up iptables rules..."
    sudo iptables -t nat -D POSTROUTING -s $SUBNET -o $MAIN_IF -j MASQUERADE 2>/dev/null || true
    sudo iptables -D FORWARD -i $VETH_HOST -o $MAIN_IF -j ACCEPT 2>/dev/null || true
    sudo iptables -D FORWARD -i $MAIN_IF -o $VETH_HOST -m state --state RELATED,ESTABLISHED -j ACCEPT 2>/dev/null || true

    echo -e "\n[+] Teardown complete."
}

# === Main entry point ===
case "${1:-}" in
    "setup")
        setup
        ;;
    "teardown")
        teardown
        ;;
    *)
        echo "[!] Error: Invalid command '$1'."
        echo -e "\nUsage: $0 <command>"
        echo "Available commands: setup, teardown"
        exit 1
        ;;
esac