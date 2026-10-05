fn main() {
    // Keep the target-native artifact embedded; rendering remains OS-owned.
    core::hint::black_box(voxygen::headless::shader::embedded_package());
    if let Err(error) = voxygen::headless::run() {
        let _ = trueos::logl::log_record(
            trueos::logl::level::ERROR,
            "apps::voxygen",
            format_args!("Headless client: {error}"),
        );
        std::process::exit(1);
    }
}
