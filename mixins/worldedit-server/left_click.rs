fn we_left_click(game: &mut crate::game::Game<'_>, pos: [i32; 3], canceled: &mut bool) {
    crate::game::worldedit_server::left_click(game, pos, canceled);
}
