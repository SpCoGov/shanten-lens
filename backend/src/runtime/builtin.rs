use super::fuse::{enabled, ints, FuseRules};
use crate::{
    pipeline::{
        Direction, ModuleAction, ModuleRegistry, Packet, PacketModule, PacketModuleInfo,
        PacketOperation, PacketSubscription, PipelineConfig, GAME_RECORD, REPLAY_INJECTOR,
    },
    services::Services,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) fn builtin_modules(
    services: Option<Arc<Services>>,
    flow_id: u64,
) -> Vec<Box<dyn PacketModule>> {
    vec![
        Box::new(MethodFilter),
        Box::new(GameRecord(services.clone())),
        Box::new(PacketLogger(services.clone())),
        Box::new(GameState(services.clone(), flow_id)),
        Box::new(UnlockIllustratedBook(services.clone())),
        Box::new(FuseRules(services.clone())),
        Box::new(AutorunEvents(services)),
    ]
}

pub fn register_builtin_modules(registry: &ModuleRegistry, config: &PipelineConfig) {
    let _ = registry.register(PacketModuleInfo::builtin(
        REPLAY_INJECTOR,
        vec![subscription(
            Some(Direction::Outbound),
            Some("Req"),
            None,
            &[PacketOperation::Inject],
        )],
    ));
    for module in builtin_modules(None, 0) {
        let options = config
            .modules
            .iter()
            .find(|entry| entry.id == module.id())
            .map(|entry| &entry.options)
            .unwrap_or(&Value::Null);
        let _ = registry.register(module.registration(options));
    }
}

pub(super) fn subscription(
    direction: Option<Direction>,
    packet_type: Option<&str>,
    method: Option<&str>,
    operations: &[PacketOperation],
) -> PacketSubscription {
    PacketSubscription {
        direction,
        packet_type: packet_type.map(str::to_owned),
        method: method.map(str::to_owned),
        operations: operations.to_vec(),
    }
}

pub(super) fn method_subscriptions(
    direction: Direction,
    packet_type: &str,
    methods: &[&str],
    operations: &[PacketOperation],
) -> Vec<PacketSubscription> {
    methods
        .iter()
        .map(|method| subscription(Some(direction), Some(packet_type), Some(method), operations))
        .collect()
}

struct PacketLogger(Option<Arc<Services>>);
impl PacketModule for PacketLogger {
    fn id(&self) -> &'static str {
        "packet_logger"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![subscription(None, None, None, &[PacketOperation::Read])]
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            services.record_packet(packet);
        }
        ModuleAction::Forward
    }
}

struct GameRecord(Option<Arc<Services>>);
impl PacketModule for GameRecord {
    fn id(&self) -> &'static str {
        GAME_RECORD
    }

    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![
            subscription(
                Some(Direction::Inbound),
                Some("Res"),
                Some(".lq.Lobby.fetchGameRecord"),
                &[PacketOperation::Read, PacketOperation::Edit],
            ),
            subscription(
                Some(Direction::Outbound),
                Some("Req"),
                Some(".lq.Lobby.fetchGameRecord"),
                &[PacketOperation::Inject],
            ),
        ]
    }

    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            if !services.packet_processing_allowed() { return ModuleAction::Forward; }
            services.apply_game_record_override(packet);
            services.publish_game_record(packet);
        }
        ModuleAction::Forward
    }
}

struct GameState(Option<Arc<Services>>, u64);
impl PacketModule for GameState {
    fn id(&self) -> &'static str {
        "game_state"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        vec![
            subscription(
                Some(Direction::Inbound),
                None,
                None,
                &[PacketOperation::Read],
            ),
            subscription(
                Some(Direction::Inbound),
                Some("Res"),
                None,
                &[PacketOperation::Edit],
            ),
        ]
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            services.update_game_state_from_flow(packet, self.1);
            apply_game_response_options(packet, services);
        }
        ModuleAction::Forward
    }
}

fn apply_game_response_options(packet: &mut Packet, services: &Services) {
    if !services.packet_processing_allowed() { return; }
    if packet.direction != Direction::Inbound || packet.packet_type != "Res" {
        return;
    }
    let game = services.table("game");
    if packet.method == ".lq.Lobby.fetchAnnouncement" && enabled(&game, "modify_announcement", true)
    {
        if let Some(items) = packet
            .data
            .get_mut("announcements")
            .and_then(Value::as_array_mut)
        {
            items.insert(0, services.startup_announcement());
        }
    }
    if packet.method == ".lq.Lobby.fetchAmuletActivityData" {
        let code = services
            .table("general")
            .get("error_code_test")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if code != 0 {
            packet.data =
                json!({"error":{"code":code,"u32Params":[],"strParams":[],"jsonParam":""}});
            return;
        }
    }
    if enabled(&game, "public_all", false) {
        let state = services.game_state();
        let wall = ints(state.get("wall_tiles"));
        let locked = ints(state.get("locked_tiles"));
        let mut pos = (wall.len() + locked.len()) as i64 - 1;
        let mut tiles = Vec::with_capacity(wall.len() + locked.len());
        for id in wall.into_iter().chain(locked) {
            tiles.push(json!({"id":id,"pos":pos}));
            pos -= 1
        }
        replace_desktop_tiles(&mut packet.data, &tiles);
    }
}

struct UnlockIllustratedBook(Option<Arc<Services>>);
impl PacketModule for UnlockIllustratedBook {
    fn id(&self) -> &'static str {
        "unlock_illustrated_book"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Inbound,
            "Res",
            &[".lq.Lobby.amuletActivityFetchBrief"],
            &[PacketOperation::Read, PacketOperation::Edit],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        let Some(services) = &self.0 else {
            return ModuleAction::Forward;
        };
        if packet.direction == Direction::Inbound
            && packet.packet_type == "Res"
            && packet.method == ".lq.Lobby.amuletActivityFetchBrief"
            && enabled(&services.table("game"), "unlock_illustrated_book", false)
        {
            if let Some(book) = packet.data.get_mut("illustratedBook") {
                book["effectCollection"] = json!((1..500)
                    .flat_map(|id| [id * 10, id * 10 + 1])
                    .collect::<Vec<_>>());
                book["badgeCollection"] = json!((600000..600500).collect::<Vec<_>>());
                book["runeStoneCollection"] = json!((7200..7300).collect::<Vec<_>>());
            }
        }
        ModuleAction::Forward
    }
}

fn replace_desktop_tiles(value: &mut Value, tiles: &[Value]) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "showDesktopTiles" {
                    if child.is_array() {
                        *child = Value::Array(tiles.to_vec())
                    } else if let Some(inner) = child.get_mut("value") {
                        *inner = Value::Array(tiles.to_vec())
                    }
                } else {
                    replace_desktop_tiles(child, tiles)
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                replace_desktop_tiles(item, tiles)
            }
        }
        _ => {}
    }
}

struct AutorunEvents(Option<Arc<Services>>);
impl PacketModule for AutorunEvents {
    fn id(&self) -> &'static str {
        "autorun"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Inbound,
            "Res",
            &[
                ".lq.Lobby.amuletActivityStartGame",
                ".lq.Lobby.amuletActivityGiveup",
                ".lq.Lobby.amuletActivityOperate",
                ".lq.Lobby.amuletActivityGameOperate",
                ".lq.Lobby.amuletActivityEndShopping",
            ],
            &[PacketOperation::Read],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        if let Some(services) = &self.0 {
            if packet.direction == Direction::Inbound
                && packet.packet_type == "Res"
                && matches!(
                    packet.method.as_str(),
                    ".lq.Lobby.amuletActivityStartGame"
                        | ".lq.Lobby.amuletActivityGiveup"
                        | ".lq.Lobby.amuletActivityOperate"
                        | ".lq.Lobby.amuletActivityGameOperate"
                        | ".lq.Lobby.amuletActivityEndShopping"
                )
            {
                let _ = services.events.send(crate::services::event(
                    "autorun_packet",
                    json!({"method":packet.method,"id":packet.id,"data":packet.data}),
                ));
            }
        }
        ModuleAction::Forward
    }
}

struct MethodFilter;
impl PacketModule for MethodFilter {
    fn id(&self) -> &'static str {
        "method_filter"
    }
    fn subscriptions(&self, options: &Value) -> Vec<PacketSubscription> {
        options
            .get("bypass_methods")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|method| subscription(None, None, Some(method), &[PacketOperation::Bypass]))
            .collect()
    }
    fn process(&mut self, packet: &mut Packet, options: &Value) -> ModuleAction {
        let bypass = options
            .get("bypass_methods")
            .and_then(Value::as_array)
            .is_some_and(|methods| {
                methods
                    .iter()
                    .any(|method| method.as_str() == Some(&packet.method))
            });
        if bypass {
            ModuleAction::Bypass
        } else {
            ModuleAction::Forward
        }
    }
}

pub fn default_module_options(id: &str) -> Value {
    match id {
        "method_filter" => serde_json::json!({"bypass_methods": [".lq.Route.heartbeat"]}),
        _ => Value::Null,
    }
}

pub fn normalize_config(mut config: PipelineConfig) -> PipelineConfig {
    // The activity module is now provided by the standalone activity plugin.
    config
        .modules
        .retain(|module| module.id != "limited_time_activity");
    let defaults = PipelineConfig::default();
    for module in &mut config.modules {
        if module.options.is_null() {
            module.options = default_module_options(&module.id);
        }
    }
    for module in defaults.modules {
        if !config.modules.iter().any(|known| known.id == module.id) {
            if module.id == REPLAY_INJECTOR {
                config.modules.insert(0, module);
            } else {
                config.modules.push(module);
            }
        }
    }
    config
}
