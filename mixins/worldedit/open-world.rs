// Entering a world makes its schematics folder, so the menu never lists a
// folder that is not there, and notes the world for the chat completions.
fn open_world_head(folder: &str) {
    let world = std::env::current_dir()
        .unwrap_or_default()
        .join("saves")
        .join(folder);
    horizon_client::chat::worldedit_paths::ensure_world(&world);
}
