print "Creating test interfaces..."

if (ip link show test_iface1 | complete | get exit_code) != 0 and (ip link show test_iface2 | complete | get exit_code) != 0 {
    # Create a veth pair
    sudo ip link add test_iface1 type veth peer name test_iface2

    # Set interfaces up
    ip link set test_iface1 up
    ip link set test_iface2 up

    # Assign IP addresses
    sudo ip addr add 192.168.10.1/24 dev test_iface1
    sudo ip addr add 192.168.10.2/24 dev test_iface2

    sudo ip link set dev test_iface1 address 2a:2b:72:fb:e8:cc 
    sudo ip link set dev test_iface2 address ea:d8:f6:0e:76:01

    # Optional: Configure specific queue parameters if needed
    # ethtool -L test_iface1 combined 1 or true
    # ethtool -L test_iface2 combined 1 or true

    # Enable XDP on the interfaces
    # ip link set dev test_iface1 xdp generic on
    # ip link set dev test_iface2 xdp generic on

    print "Test interfaces created and configured:"
    ip addr show test_iface1
    ip addr show test_iface2
}

print "Network environment setup complete!"
