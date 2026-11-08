// Keys while the menu is up: Esc closes it, Enter loads the pick, the arrows
// and paging move through the list, and nothing else reaches the game.
fn schem_key_head(
    game: &mut crate::game::Game<'_>,
    key: winit::keyboard::KeyCode,
) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::worldedit::menu_key(game, key) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
