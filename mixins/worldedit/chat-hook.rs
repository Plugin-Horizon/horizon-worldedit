fn chat_suggestions_head(
    screen: &mut crate::chat::ChatScreen,
    m: &crate::screen_draw::Measure,
    c: &crate::chat::Commands<'_>,
) -> core::ops::ControlFlow<()> {
    if crate::chat::worldedit_chat::suggest(screen, m, c) {
        core::ops::ControlFlow::Break(())
    } else {
        core::ops::ControlFlow::Continue(())
    }
}
