// The editor's core crate, re-exported from horizon-client-lib as `horizon_client::worldedit_server::core`:
// the game side (glue.rs, compiled into the game executable) and the `[api]` package (which depends on
// horizon-client's library only) both reach the one engine and its sessions through it.
pub use horizon_worldedit_core as core;
