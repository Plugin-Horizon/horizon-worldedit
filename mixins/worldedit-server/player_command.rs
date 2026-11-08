fn we_player_command(
    game: &mut crate::game::Game<'_>,
    player: u128,
    name: &str,
    command: &str,
    canceled: &mut bool,
    replies: &mut Vec<horizon_client::text::Text>,
) {
    crate::game::worldedit_server::player_command(game, player, name, command, canceled, replies);
}
