// A left click while the menu is up is the menu's: the pause menu's buttons
// are not under it.
fn schem_click_head(game: &mut crate::game::Game<'_>) -> core::ops::ControlFlow<Result<()>> {
    if crate::game::worldedit::menu_click(game) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
