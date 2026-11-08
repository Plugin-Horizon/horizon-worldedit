fn we_level_ticked(game: &mut crate::game::Game<'_>, dimension: &str) {
    crate::game::worldedit_server::level_ticked(game, dimension);
}
