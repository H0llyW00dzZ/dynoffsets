//! Pattern-based discovery of engine globals (dw_* pointers).

use crate::sigscan::{find_pattern_rip32, find_pattern_u32, find_pattern_u8};

/// RIP-resolved address, converted to a module-relative offset.
fn find_offset_rip32(module: &str, pattern: &[Option<u8>], disp_off: usize) -> Option<usize> {
    let abs = find_pattern_rip32(module, pattern, disp_off)?;
    let base = crate::process()?.module_base(module)?;
    abs.checked_sub(base)
}

/// Live globals from pattern scan; None on signature miss.
#[derive(Debug, Default, Clone)]
pub struct RuntimeGlobals {
    pub dw_csgo_input: Option<usize>,
    pub dw_entity_list: Option<usize>,
    pub dw_game_entity_system: Option<usize>,
    pub dw_game_entity_system_highest_entity_index: Option<usize>,
    pub dw_game_rules: Option<usize>,
    pub dw_global_vars: Option<usize>,
    pub dw_glow_manager: Option<usize>,
    pub dw_local_player_controller: Option<usize>,
    pub dw_local_player_pawn: Option<usize>,
    pub dw_planted_c4: Option<usize>,
    pub dw_prediction: Option<usize>,
    pub dw_sensitivity: Option<usize>,
    pub dw_sensitivity_sensitivity: Option<usize>,
    pub dw_view_angles: Option<usize>,
    pub dw_view_matrix: Option<usize>,
    pub dw_view_render: Option<usize>,
    pub dw_weapon_c4: Option<usize>,

    pub dw_build_number: Option<usize>,
    pub dw_network_game_client: Option<usize>,
    pub dw_network_game_client_client_tick_count: Option<usize>,
    pub dw_network_game_client_delta_tick: Option<usize>,
    pub dw_network_game_client_is_background_map: Option<usize>,
    pub dw_network_game_client_local_player: Option<usize>,
    pub dw_network_game_client_max_clients: Option<usize>,
    pub dw_network_game_client_server_tick_count: Option<usize>,
    pub dw_network_game_client_sign_on_state: Option<usize>,
    pub dw_window_height: Option<usize>,
    pub dw_window_width: Option<usize>,

    pub dw_input_system: Option<usize>,
    pub dw_game_types: Option<usize>,
    pub dw_sound_system: Option<usize>,
    pub dw_sound_system_engine_view_data: Option<usize>,
}

impl RuntimeGlobals {
    /// Lookup by name. Used by `r#static` globals via `populate`.
    pub fn get(&self, name: &str) -> Option<usize> {
        match name {
            "dw_csgo_input" => self.dw_csgo_input,
            "dw_entity_list" => self.dw_entity_list,
            "dw_game_entity_system" => self.dw_game_entity_system,
            "dw_game_entity_system_highest_entity_index" => {
                self.dw_game_entity_system_highest_entity_index
            }
            "dw_game_rules" => self.dw_game_rules,
            "dw_global_vars" => self.dw_global_vars,
            "dw_glow_manager" => self.dw_glow_manager,
            "dw_local_player_controller" => self.dw_local_player_controller,
            "dw_local_player_pawn" => self.dw_local_player_pawn,
            "dw_planted_c4" => self.dw_planted_c4,
            "dw_prediction" => self.dw_prediction,
            "dw_sensitivity" => self.dw_sensitivity,
            "dw_sensitivity_sensitivity" => self.dw_sensitivity_sensitivity,
            "dw_view_angles" => self.dw_view_angles,
            "dw_view_matrix" => self.dw_view_matrix,
            "dw_view_render" => self.dw_view_render,
            "dw_weapon_c4" => self.dw_weapon_c4,
            "dw_build_number" => self.dw_build_number,
            "dw_network_game_client" => self.dw_network_game_client,
            "dw_network_game_client_client_tick_count" => {
                self.dw_network_game_client_client_tick_count
            }
            "dw_network_game_client_delta_tick" => self.dw_network_game_client_delta_tick,
            "dw_network_game_client_is_background_map" => {
                self.dw_network_game_client_is_background_map
            }
            "dw_network_game_client_local_player" => self.dw_network_game_client_local_player,
            "dw_network_game_client_max_clients" => self.dw_network_game_client_max_clients,
            "dw_network_game_client_server_tick_count" => {
                self.dw_network_game_client_server_tick_count
            }
            "dw_network_game_client_sign_on_state" => self.dw_network_game_client_sign_on_state,
            "dw_window_height" => self.dw_window_height,
            "dw_window_width" => self.dw_window_width,
            "dw_input_system" => self.dw_input_system,
            "dw_game_types" => self.dw_game_types,
            "dw_sound_system" => self.dw_sound_system,
            "dw_sound_system_engine_view_data" => self.dw_sound_system_engine_view_data,
            _ => None,
        }
    }
}

macro_rules! b {
    (?) => {
        None
    };
    ($x:literal) => {
        Some($x)
    };
}
macro_rules! sig { ($($t:tt)*) => { &[$(b!($t)),*] }; }

/// Discover engine globals via patterns (for #[globals] macro).
pub fn discover_globals() -> RuntimeGlobals {
    // Some people on UC will call this "AI slop" just because the line is long lol.
    // Funny how blind some of them are.
    let dw_csgo_input = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x89 0x05 ? ? ? ? 0x0F 0x57 0xC0 0x0F 0x11 0x05),
        3,
    );
    let dw_view_angles = dw_csgo_input.and_then(|csgo_input| {
        find_pattern_u32("client.dll", sig!(0xF2 0x42 0x0F 0x10 0x84 0x28 ? ? ? ?), 6)
            .map(|i| csgo_input.wrapping_add(i as usize))
    });
    let dw_entity_list = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x89 0x0D ? ? ? ? 0xE9 ? ? ? ? 0xCC),
        3,
    );
    let dw_game_entity_system = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8B 0x1D ? ? ? ? 0x48 0x89 0x1D ? ? ? ? 0x4C 0x63 0xB3),
        3,
    );
    let dw_game_entity_system_highest_entity_index =
        find_pattern_u32("client.dll", sig!(0xFF 0x81 ? ? ? ? 0x48 0x85 0xD2), 2)
            .map(|v| v as usize);
    let dw_game_rules = find_offset_rip32(
        "client.dll",
        sig!(0xF6 0xC1 0x01 0x0F 0x85 ? ? ? ? 0x4C 0x8B 0x05 ? ? ? ? 0x4D 0x85),
        12,
    );
    let dw_global_vars =
        find_offset_rip32("client.dll", sig!(0x48 0x89 0x15 ? ? ? ? 0x48 0x89 0x42), 3);

    let dw_glow_manager = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8B 0x05 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x8B 0x41),
        3,
    );
    let dw_local_player_controller =
        find_offset_rip32("client.dll", sig!(0x48 0x8B 0x05 ? ? ? ? 0x41 0x89 0xBE), 3);
    let dw_planted_c4 = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8B 0x15 ? ? ? ? 0x41 0xFF 0xC0 0x48 0x8D 0x4C 0x24),
        3,
    );
    let dw_prediction = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8D 0x05 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x40 0x53 0x56 0x41 0x54),
        3,
    );
    let dw_local_player_pawn = dw_prediction.and_then(|prediction| {
        find_pattern_u32(
            "client.dll",
            sig!(0x4C 0x39 0xB6 ? ? ? ? 0x74 ? 0x44 0x88 0xBE),
            3,
        )
        .map(|i| prediction.wrapping_add(i as usize))
    });

    let dw_sensitivity = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8D 0x0D ? ? ? ? 0x66 0x0F 0x6E 0xCD),
        3,
    );
    let dw_sensitivity_sensitivity = find_pattern_u8(
        "client.dll",
        sig!(0x48 0x8D 0x7E ? 0x48 0x0F 0xBA 0xE0 ? 0x72 ? 0x85 0xD2 0x49 0x0F 0x4F 0xFF),
        3,
    )
    .map(|v| v as usize);

    let dw_view_matrix = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8D 0x0D ? ? ? ? 0x48 0xC1 0xE0 0x06),
        3,
    );
    let dw_view_render = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x89 0x05 ? ? ? ? 0x48 0x8B 0xC8 0x48 0x85 0xC0),
        3,
    );
    let dw_weapon_c4 = find_offset_rip32(
        "client.dll",
        sig!(0x48 0x8B 0x15 ? ? ? ? 0x48 0x8B 0x5C 0x24 ? 0xFF 0xC0 0x89 0x05 ? ? ? ? 0x48 0x8B 0xC6 0x48 0x89 0x34 0xEA 0x80 0xBE),
        3,
    );

    let dw_build_number = find_offset_rip32(
        "engine2.dll",
        sig!(0x89 0x05 ? ? ? ? 0x48 0x8D 0x0D ? ? ? ? 0xFF 0x15 ? ? ? ? 0x48 0x8B 0x0D),
        2,
    );
    let dw_network_game_client =
        find_offset_rip32("engine2.dll", sig!(0x48 0x89 0x3D ? ? ? ? 0xFF 0x87), 3);
    let dw_network_game_client_client_tick_count = find_pattern_u32(
        "engine2.dll",
        sig!(0x8B 0x81 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x8B 0x81 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x83 0xB9),
        2,
    )
    .map(|v| v as usize);
    let dw_network_game_client_delta_tick = find_pattern_u32(
        "engine2.dll",
        sig!(0x4C 0x8D 0xB7 ? ? ? ? 0x4C 0x89 0x7C 0x24),
        3,
    )
    .map(|v| v as usize);
    // `is_background_map` is referenced as "movzx eax, byte ptr [rcx+disp32]"
    let dw_network_game_client_is_background_map = find_pattern_u32(
        "engine2.dll",
        sig!(0x0F 0xB6 0x81 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x0F 0xB6 0x81 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x40 0x53),
        3,
    )
    .map(|v| v as usize);
    let dw_network_game_client_local_player = find_pattern_u32(
        "engine2.dll",
        sig!(0x42 0x8B 0x94 0xD3 ? ? ? ? 0x5B 0x49 0xFF 0xE3 0x32 0xC0 0x5B 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x40 0x53),
        4,
    )
    .map(|v| v as usize);
    let dw_network_game_client_max_clients = find_pattern_u32(
        "engine2.dll",
        sig!(0x8B 0x81 ? ? ? ? 0xC3 ? ? ? ? ? ? ? ? ? 0x8B 0x81 ? ? ? ? 0xC3 ? ? ? ? ? ? ? ? ? 0x8B 0x81),
        2,
    )
    .map(|v| v as usize);
    let dw_network_game_client_server_tick_count = find_pattern_u32(
        "engine2.dll",
        sig!(0x8B 0x81 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x83 0xB9),
        2,
    )
    .map(|v| v as usize);
    let dw_network_game_client_sign_on_state = find_pattern_u32(
        "engine2.dll",
        sig!(0x44 0x8B 0x81 ? ? ? ? 0x48 0x8D 0x0D),
        3,
    )
    .map(|v| v as usize);
    let dw_window_height = find_offset_rip32("engine2.dll", sig!(0x8B 0x05 ? ? ? ? 0x89 0x03), 2);
    let dw_window_width = find_offset_rip32("engine2.dll", sig!(0x8B 0x05 ? ? ? ? 0x89 0x07), 2);

    let dw_input_system =
        find_offset_rip32("inputsystem.dll", sig!(0x48 0x89 0x05 ? ? ? ? 0x33 0xC0), 3);
    let dw_game_types =
        find_offset_rip32("matchmaking.dll", sig!(0x48 0x8D 0x0D ? ? ? ? 0xFF 0x90), 3);
    let dw_sound_system = find_offset_rip32(
        "soundsystem.dll",
        sig!(0x48 0x8D 0x05 ? ? ? ? 0xC3 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0xCC 0x48 0x89 0x15),
        3,
    );
    let dw_sound_system_engine_view_data = find_pattern_u8(
        "soundsystem.dll",
        sig!(0x0F 0x11 0x47 ? 0x0F 0x10 0x4E ? 0x0F 0x11 0x8F),
        3,
    )
    .map(|v| v as usize);

    RuntimeGlobals {
        dw_csgo_input,
        dw_entity_list,
        dw_game_entity_system,
        dw_game_entity_system_highest_entity_index,
        dw_game_rules,
        dw_global_vars,
        dw_glow_manager,
        dw_local_player_controller,
        dw_local_player_pawn,
        dw_planted_c4,
        dw_prediction,
        dw_sensitivity,
        dw_sensitivity_sensitivity,
        dw_view_angles,
        dw_view_matrix,
        dw_view_render,
        dw_weapon_c4,

        dw_build_number,
        dw_network_game_client,
        dw_network_game_client_client_tick_count,
        dw_network_game_client_delta_tick,
        dw_network_game_client_is_background_map,
        dw_network_game_client_local_player,
        dw_network_game_client_max_clients,
        dw_network_game_client_server_tick_count,
        dw_network_game_client_sign_on_state,
        dw_window_height,
        dw_window_width,

        dw_input_system,
        dw_game_types,
        dw_sound_system,
        dw_sound_system_engine_view_data,
    }
}
