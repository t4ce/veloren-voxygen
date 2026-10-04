#[cfg(windows)]
fn main() {
    assert!(std::env::var_os("CARGO_FEATURE_SINGLEPLAYER").is_none(), "Singleplayer is unavailable in this client-only workspace");
    //Set executable logo with winres here:
    let mut res = winres::WindowsResource::new();
    res.set_icon("../assets/voxygen/logo.ico");
    res.compile().expect("failed to build executable logo.");
}

#[cfg(not(windows))]
fn main() {
    assert!(std::env::var_os("CARGO_FEATURE_SINGLEPLAYER").is_none(), "Singleplayer is unavailable in this client-only workspace");}
