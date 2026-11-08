fn we_register_config(list: &mut Vec<crate::game::native_configs::NativeConfig>) {
    list.push(
        crate::game::native_configs::NativeConfig::new(
            crate::game::worldedit_server::CONFIG_ID,
            crate::game::native_configs::NativeConfigKind::Server,
            horizon_worldedit_core::config::DEFAULT,
        )
        .file(crate::game::worldedit_server::CONFIG_FILE),
    );
}
