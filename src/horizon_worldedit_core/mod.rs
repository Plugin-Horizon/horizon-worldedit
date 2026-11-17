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
pub mod complete;
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
pub mod yaml; // vendored horizon-yaml: reads the original plugins/WorldEdit/config.yml

pub use block::BlockState;
pub use config::Config;
pub use engine::{form_allows, is_command, is_rotate, is_shadowed, usages, Actor, Engine, Env, Line, Reply, NODES};
pub use world::{Pos, StateId, World};

use std::collections::BTreeSet;
use std::path::Path;
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

/// The original WorldEdit folder, `plugins/WorldEdit` (read when it has a `config.yml`, in place of
/// `config/worldedit.toml`): `config.yml` gives the settings, and `sessions/<player>.json` the players'
/// `useServerCUI`. Returns what is wrong with either, if anything. Nothing is written.
pub fn configure_original(plugin_dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    match Config::read_original(plugin_dir) {
        Ok((config, _)) => with(|e| e.config = config),
        Err(e) => problems.push(format!("{e}; keeping the settings in force")),
    }
    let mut cui_off = BTreeSet::new();
    let entries = std::fs::read_dir(plugin_dir.join("sessions")).into_iter().flatten().flatten();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // The file name is the player's UUID; a file without one is said so, not named.
        let Some(player) = path.file_stem().and_then(|s| s.to_str()).and_then(session::uuid_from_stem) else {
            problems.push("a session file does not name a player's UUID; skipped".into());
            continue;
        };
        match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| session::use_server_cui(&t)) {
            Ok(true) => {}
            Ok(false) => {
                cui_off.insert(player);
            }
            Err(e) => problems.push(format!("a session file is not read: {e}")),
        }
    }
    with(|e| e.set_cui_off(cui_off));
    problems
}

#[cfg(test)]
mod tests;
