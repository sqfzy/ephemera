nu tests/setup_net.nu

RUST_LOG=trace sudo -E (which cargo | get path | get 0) nextest run --no-capture

nu tests/cleanup_net.nu
