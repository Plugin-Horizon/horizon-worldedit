fn we_use_item(game: &mut crate::game::Game<'_>, stack: &horizon_client::items::ItemStack, claimed: &mut bool) {
    crate::game::worldedit_server::use_item(game, &stack.item, claimed);
}
