fn main() {
    // Keep the target-native artifact embedded; rendering remains OS-owned.
    core::hint::black_box(voxygen::headless::shader::embedded_package());
    if let Err(error) = voxygen::headless::run() {
        eprintln!("Headless client: {error}");
        std::process::exit(1);
    }
}
