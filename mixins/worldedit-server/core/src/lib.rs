//! WorldEdit for a Horizon dedicated server: everything that needs no game types. The bundle's
//! glue (`glue.rs`, staged into the game) implements [`World`] over the level in place and calls
//! [`with`] from the hook points.
//!
//! A fork of XnotCykoX/WorldEdit-Horizon (MIT): the upstream bundle edits the world of the window
//! it runs in; this one runs where a server simulates the world, for every player, through Horizon's
//! native server hooks. Derived from WorldEdit (https://github.com/EngineHub/WorldEdit, GPL-3.0):
//! its commands, permission nodes, chat messages and algorithms are ported from the source. Not
//! affiliated with EngineHub.
pub mod block;
pub mod clipboard;
pub mod config;
pub mod edit;
pub mod engine;
pub mod nbt;
pub mod ops;
pub mod pattern;
pub mod region;
pub mod schem;
pub mod session;
pub mod shapes;
pub mod world;

pub use block::BlockState;
pub use config::Config;
pub use engine::{is_command, is_rotate, is_shadowed, usages, Actor, Engine, Env, Line, Reply};
pub use world::{Pos, StateId, World};

use std::sync::Mutex;

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// Runs `f` with the server's engine (made with the default settings on first use).
pub fn with<R>(f: impl FnOnce(&mut Engine) -> R) -> R {
    let mut guard = ENGINE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Engine::default))
}

/// `config/worldedit.toml` was read (or read again): the new settings, and what was wrong with
/// the file, if anything.
pub fn configure(text: &str) -> Option<String> {
    let (config, problem) = Config::parse(text);
    with(|e| e.config = config);
    problem
}

#[cfg(test)]
mod tests;
