fn wand_head(
    game: &mut crate::game::Game<'_>,
    button: winit::event::MouseButton,
    pressed: bool,
) -> core::ops::ControlFlow<Result<()>> {
    if game.worldedit_wand(button, pressed) {
        core::ops::ControlFlow::Break(Ok(()))
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
