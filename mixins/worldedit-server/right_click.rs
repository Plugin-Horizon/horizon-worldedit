fn we_right_click(
    game: &mut crate::game::Game<'_>,
    pos: [i32; 3],
    canceled: &mut bool,
) {
    crate::game::worldedit_server::right_click(game, pos, canceled);
}
