// The pause menu's draw is the menu's while one is open; otherwise the pause
// menu draws itself, untouched.
fn schem_draw_head(
    game: &mut crate::game::Game<'_>,
    batch: &mut horizon_client::hud::HudBatch,
) -> core::ops::ControlFlow<()> {
    if crate::game::worldedit::menu_draw(game, batch) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
