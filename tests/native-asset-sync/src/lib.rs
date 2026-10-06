//! Exercises the production sync worker on the host with real redb and TCP.
#![allow(dead_code)]
extern crate alloc;
extern crate self as common;
pub mod assets { pub use veloren_common_assets::*; }
pub mod client {
    pub mod addr {
        pub(crate) async fn resolve(address: &str, _: bool) -> std::io::Result<Vec<std::net::SocketAddr>> {
            match tokio::net::lookup_host(address).await {
                Ok(hosts) => Ok(hosts.collect()),
                Err(_) => Ok(tokio::net::lookup_host((address, 14004)).await?.collect()),
            }
        }
    }
}
#[path = "../../../src/asset_sync.rs"]
mod production;
