fn command_head(game: &mut crate::game::Game<'_>, command: &str) -> core::ops::ControlFlow<bool> {
    if game.worldedit_defers_to_server() {
        return core::ops::ControlFlow::Continue(());
    }
    match game.worldedit_command(command) {
        Some(result) => core::ops::ControlFlow::Break(result),
        None => core::ops::ControlFlow::Continue(()),
    }
}
