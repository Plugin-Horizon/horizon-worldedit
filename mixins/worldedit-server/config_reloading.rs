fn we_config_reloading(file: &crate::game::native_configs::NativeConfigFile) {
    if file.mod_id == crate::game::worldedit_server::CONFIG_ID {
        crate::game::worldedit_server::configure(&file.text);
    }
}
