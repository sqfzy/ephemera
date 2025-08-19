# --- Configuration Variables ---
# Use a NEW subnet for the virtual network
let VIRTUAL_SUBNET = "192.168.10.0/24"
let BRIDGE_IP = "192.168.10.1/24"
let VETH_IP = "192.168.10.10/24"

# Your REAL internet interface
let OUT_IF = "eth1"

# --- Create Virtual Devices ---
# 1. Create the bridge
sudo ip link add name br0 type bridge

# 2. Create the veth pair
sudo ip link add name veth0 type veth peer name veth1

# 3. Connect veth1 to the bridge
sudo ip link set veth1 master br0

# --- Configure IP Addresses ---
# 4. Assign the GATEWAY IP to the bridge
sudo ip addr add $BRIDGE_IP dev br0

# 5. Assign the CLIENT IP to veth0
sudo ip addr add $VETH_IP dev veth0

# --- Bring Interfaces Up ---
# 6. Activate all the new interfaces
sudo ip link set br0 up
sudo ip link set veth1 up
sudo ip link set veth0 up

# --- Configure Routing & Firewall (The CRITICAL Part) ---
# 7. Add a route for veth0 to use the bridge as its gateway
# sudo ip route add default via 192.168.10.1 dev veth0

# 8. Set up iptables to NAT traffic from the virtual subnet to the internet
sudo iptables -t nat -A POSTROUTING -s $VIRTUAL_SUBNET -o $OUT_IF -j MASQUERADE
sudo iptables -A FORWARD -i br0 -o $OUT_IF -j ACCEPT
sudo iptables -A FORWARD -i $OUT_IF -o br0 -m state --state RELATED,ESTABLISHED -j ACCEPT
