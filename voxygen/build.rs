extern crate alloc;
fn main() {
    assert!(
        std::env::var_os("CARGO_FEATURE_SINGLEPLAYER").is_none(),
        "Singleplayer is unavailable in this client-only repository"
    );
}
