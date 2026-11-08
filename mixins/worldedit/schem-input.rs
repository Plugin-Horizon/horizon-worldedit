// `//schem` typed in the chat screen opens the menu itself: on a world this
// window owns, the message never reaches the server. Everything else — a
// guest's world, `//schem list`, `//schem load <file>` — goes on its way.
fn schem_input_head(game: &mut crate::game::Game<'_>, message: &str) -> core::ops::ControlFlow<()> {
    if crate::game::worldedit::schem_input(game, message) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
