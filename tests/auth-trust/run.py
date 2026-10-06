#!/usr/bin/env python3
"""Check saved networking defaults and the TRUEOS login callback.

Only serde derives are removed and the host HashSet replaces hashbrown.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / "src/settings/networking.rs").read_text()
source = source.replace("use hashbrown::HashSet;", "use std::collections::HashSet;")
source = source.replace("use serde::{Deserialize, Serialize};", "")
source = source.replace(
    "#[derive(Clone, Debug, Serialize, Deserialize)]", "#[derive(Clone, Debug)]"
).replace("#[serde(default)]", "")
source += r'''
#[test]
fn official_https_provider_survives_an_empty_saved_trust_list() {
    let mut settings = NetworkingSettings::default();
    settings.trusted_auth_servers.clear();
    assert!(settings.is_auth_server_trusted("https://auth.veloren.net"));
}
#[test]
fn builtin_trust_does_not_extend_to_lookalikes_or_insecure_addresses() {
    let mut settings = NetworkingSettings::default();
    settings.trusted_auth_servers.clear();
    for server in ["http://auth.veloren.net", "https://auth.veloren.net.evil.example",
                   "https://evil.example/auth.veloren.net", "https://auth.veloren.net@evil.example",
                   "https://auth.veloren.net:8443", "https://other.example"] {
        assert!(!settings.is_auth_server_trusted(server), "unexpected built-in trust: {server}");
    }
}
#[test]
fn explicitly_trusted_other_provider_is_remembered() {
    let mut settings = NetworkingSettings::default();
    let server = "https://custom.example";
    assert!(!settings.is_auth_server_trusted(server));
    settings.trusted_auth_servers.insert(server.into());
    assert!(settings.is_auth_server_trusted(server));
    assert!(!settings.is_auth_server_trusted("https://custom.example.evil.example"));
}
'''
initializer = (ROOT / "src/menu/main/client_init.rs").read_text()
callback = initializer.split("runtime.spawn(async move {", 1)[1].split("let mut last_err", 1)[0]
flow_test = r'''
#![allow(dead_code, unused_variables)]
struct RequestSender;
impl RequestSender {
    fn send<T>(&self, _:T) -> Result<(),()> { panic!("TRUEOS must not ask the UI for authentication trust") }
}
struct TrustReceiver;
impl TrustReceiver {
    fn recv(&self) -> Result<(),()> { panic!("TRUEOS must not wait for the trust dialog") }
}
#[test]
fn trueos_login_approves_the_supplied_service_without_a_ui_roundtrip() {
    let tx = RequestSender;
    let trust_rx = TrustReceiver;
    @CALLBACK@
    for server in ["https://auth.veloren.net", "https://another-provider.example"] {
        assert!(trust_fn(server));
    }
}
'''.replace("@CALLBACK@", callback)
with tempfile.TemporaryDirectory(prefix="voxy-auth-trust-") as directory:
    for name, code, flags in [
        ("settings", source, []),
        ("trueos-login", flow_test,
         ["--cfg", 'target_os="trueos"', "-A", "explicit_builtin_cfgs_in_flags"]),
    ]:
        rust = Path(directory) / f"{name}.rs"
        binary = Path(directory) / name
        rust.write_text(code)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", *flags, str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)
