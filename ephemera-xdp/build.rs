use libbpf_cargo::SkeletonBuilder;
use std::path::Path;

// Define the path to your BPF C source code.
const BPF_SRC_PATH: &str = "src/bpf/xdp_filter.bpf.c";

fn main() {
    // Tell Cargo to re-run the build script if the BPF C source file changes.
    // This is crucial for development.
    println!("cargo:rerun-if-changed={BPF_SRC_PATH}");

    // Define where the generated Rust skeleton file will be placed.
    // It will be inside the `target` directory, managed by Cargo.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let skel_path = Path::new(&out_dir).join("xdp_ip_filter.skel.rs");

    // Use SkeletonBuilder to compile the BPF code and generate the Rust module.
    SkeletonBuilder::new()
        .source(BPF_SRC_PATH)
        .build_and_generate(&skel_path)
        .expect("Failed to compile and generate BPF skeleton");
}
