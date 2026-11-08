fn we_perform(
    game: &mut crate::game::Game<'_>,
    player: &crate::game::native_server::NativeServerPlayer,
    command: &str,
    ran: &mut bool,
) {
    crate::game::worldedit_server::perform(game, player, command, ran);
}
