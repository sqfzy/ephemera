print "Cleaning up existing test interfaces..."

# Check if test_iface1 exists and remove it
if (ip link show test_iface1 | complete | get exit_code) == 0 {
    sudo ip link delete test_iface1
    print "Removed test_iface1"
}

# Check if test_iface2 exists and remove it
if (ip link show test_iface2 | complete | get exit_code) == 0 {
    sudo ip link delete test_iface2
    print "Removed test_iface2"
}

print "Network environment cleanup complete!"
