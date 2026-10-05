use std::{io, path::PathBuf, process::Command};

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("assets/voxygen-assets.redb"));
    if args.next().is_some() || output.extension().is_none_or(|ext| ext != "redb") {
        return Err(io::Error::other("usage: voxy-assets [output.redb]"));
    }
    veloren_common_assets::prepare_picasso_asset_database(&output)?;
    let compressed = output.with_extension("redb.lz4");
    let status = Command::new("lz4")
        .args(["-q", "-f", "-B4"])
        .arg(&output)
        .arg(&compressed)
        .status()?;
    if !status.success() {
        return Err(io::Error::other("LZ4 compression failed"));
    }
    eprintln!(
        "Voxygen assets: phase=db-packed path={} bytes={}",
        compressed.display(),
        std::fs::metadata(&compressed)?.len()
    );
    Ok(())
}
