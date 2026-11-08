fn we_world_phase(phase: horizon_client::mod_api::WorldPhase) {
    use horizon_client::mod_api::WorldPhase;
    if matches!(phase, WorldPhase::WorldCloseFinal | WorldPhase::WorldCloseFailed) {
        crate::game::worldedit_server::closed();
    }
}
