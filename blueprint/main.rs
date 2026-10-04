fn main() {
    if let Err(error) = voxygen::headless::run() {
        eprintln!("Headless client: {error}");
        std::process::exit(1);
    }
}
