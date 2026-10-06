use super::builtin::method_subscriptions;
use crate::{
    pipeline::{
        Direction, ModuleAction, Packet, PacketModule, PacketOperation, PacketSubscription,
    },
    services::Services,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub(super) struct FuseRules(pub(super) Option<Arc<Services>>);
impl PacketModule for FuseRules {
    fn id(&self) -> &'static str {
        "fuse_rules"
    }
    fn subscriptions(&self, _options: &Value) -> Vec<PacketSubscription> {
        method_subscriptions(
            Direction::Outbound,
            "Req",
            &[
                ".lq.Lobby.amuletActivityGameOperate",
                ".lq.Lobby.amuletActivityOperate",
                ".lq.Lobby.amuletActivityUpgrade",
                ".lq.Lobby.amuletActivityEndShopping",
            ],
            &[PacketOperation::Read, PacketOperation::Drop],
        )
    }
    fn process(&mut self, packet: &mut Packet, _options: &Value) -> ModuleAction {
        let Some(services) = &self.0 else {
            return ModuleAction::Forward;
        };
        if packet.direction != Direction::Outbound || packet.packet_type != "Req" {
            return ModuleAction::Forward;
        }
        if !services.packet_processing_allowed() { return ModuleAction::Forward; }
        let config = services.table("fuse");
        let state = services.game_state();
        let op = packet
            .data
            .get("type")
            .and_then(Value::as_i64)
            .unwrap_or(-1);
        let effects = state
            .get("effect_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let candidates = state
            .get("candidate_effect_list")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let stage = state.get("stage").and_then(Value::as_i64).unwrap_or(-1);
        let confirm = |title: &str, message: &str, values: Value| {
            if services.confirm(title, message, values) {
                ModuleAction::Forward
            } else {
                ModuleAction::Drop
            }
        };

        if packet.method == ".lq.Lobby.amuletActivityGameOperate"
            && op == 1
            && enabled(&config, "enable_missing_hand_tile_guard", true)
        {
            let mut hand = ints(state.get("hand_tiles"));
            let played = ints(packet.data.get("tileList"));
            let mut missing = Vec::new();
            for tile in &played {
                if let Some(index) = hand.iter().position(|item| item == tile) {
                    hand.remove(index);
                } else {
                    missing.push(*tile);
                }
            }
            if !missing.is_empty() {
                return confirm(
                    "fuse.guard.missingHandTile.title",
                    "fuse.guard.missingHandTile.message",
                    json!({"missingTiles":join(&missing),"playedTiles":join(&played),"handTiles":join(&ints(state.get("hand_tiles")))}),
                );
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityGameOperate"
            && op == 8
            && enabled(&config, "enable_hanabi_win_guard", true)
            && effects
                .iter()
                .any(|item| item.get("id").and_then(Value::as_u64) == Some(2221))
            && state
                .get("ming")
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
                < 2
        {
            return confirm(
                "fuse.guard.hanabiWin.title",
                "fuse.guard.hanabiWin.message",
                json!({"hanabiId":2221,"mingCount":state.get("ming").and_then(Value::as_array).map_or(0,Vec::len)}),
            );
        }
        if packet.method == ".lq.Lobby.amuletActivityOperate" {
            if op == 3
                && matches!(stage, 4 | 5)
                && enabled(&config, "enable_ting_ready_skip_guard", true)
                && state
                    .get("ting_list")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
            {
                return confirm(
                    "fuse.guard.tingReadySkip.title",
                    "fuse.guard.tingReadySkip.message",
                    json!({}),
                );
            }
            if op == 3 && matches!(stage, 2 | 9 | 16) && enabled(&config, "enable_skip_guard", true)
            {
                let guard = &config["guard_skip_contains"];
                let watched_a = ints(guard.get("amulets"));
                let watched_b = ints(guard.get("badges"));
                let hits_a = candidates
                    .iter()
                    .filter(|item| watched_a.contains(&base(item)))
                    .count();
                let hits_b = candidates
                    .iter()
                    .filter(|item| watched_b.contains(&badge(item)))
                    .count();
                if hits_a + hits_b > 0 {
                    return confirm(
                        "fuse.guard.skipPack.title",
                        "fuse.guard.skipPack.message",
                        json!({"amuletHitCount":hits_a,"badgeHitCount":hits_b}),
                    );
                }
            }
            if op == 16 && enabled(&config, "enable_shop_force_pick", false) {
                let guard = &config["guard_skip_contains"];
                let watched_a = ints(guard.get("amulets"));
                let watched_b = ints(guard.get("badges"));
                let selected = packet
                    .data
                    .get("args")
                    .and_then(Value::as_array)
                    .and_then(|a| a.first())
                    .and_then(Value::as_u64)
                    .and_then(|i| candidates.get(i as usize));
                let any_hit = candidates.iter().any(|item| {
                    watched_a.contains(&base(item)) || watched_b.contains(&badge(item))
                });
                if any_hit
                    && selected.is_none_or(|item| {
                        !watched_a.contains(&base(item)) && !watched_b.contains(&badge(item))
                    })
                {
                    return confirm(
                        "fuse.guard.forcePick.title",
                        "fuse.guard.forcePick.message",
                        json!({"selBaseId":selected.map(base).unwrap_or(0),"selRawId":selected.and_then(|v|v.get("id")).and_then(Value::as_u64).unwrap_or(0)}),
                    );
                }
            }
            if op == 8 && enabled(&config, "enable_anti_steal_eat", true) {
                let risky = effects.windows(2).any(|pair| {
                    base(&pair[0]) == 230 && badge(&pair[0]) == 600170 && theft_like(&pair[1])
                });
                if risky {
                    return confirm(
                        "fuse.guard.kaviTheft.title",
                        "fuse.guard.kaviTheft.message",
                        json!({"protectedBadges":"600170","pairsText":""}),
                    );
                }
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityUpgrade" {
            if enabled(&config, "enable_prestart_kavi_guard", true) {
                if let Some(index) = effects
                    .iter()
                    .position(|item| base(item) == 230 && badge(item) == 600170)
                {
                    let conduction =
                        effects.iter().filter(|item| badge(item) == 600170).count() as u64;
                    let min = config
                        .get("conduction_min_count")
                        .and_then(Value::as_u64)
                        .unwrap_or(3);
                    if conduction >= min
                        && [index.checked_sub(1), Some(index + 1)]
                            .into_iter()
                            .flatten()
                            .filter_map(|i| effects.get(i))
                            .all(|item| badge(item) != 0)
                    {
                        return confirm(
                            "fuse.guard.kaviPrestartConduction.title",
                            "fuse.guard.kaviPrestartConduction.message",
                            json!({"minCnt":min,"cnt":conduction,"kaviTypeText":if effects[index].get("id").and_then(Value::as_u64).unwrap_or(0)%10==1{"P"}else{"NP"}}),
                        );
                    }
                }
            }
            if enabled(&config, "enable_kavi_plus_buffer_guard", true) {
                if let Some(index) = effects.iter().position(|item| {
                    base(item) == 230
                        && item.get("id").and_then(Value::as_u64).unwrap_or(0) % 10 == 1
                }) {
                    let adjacent = [index.checked_sub(1), Some(index + 1)]
                        .into_iter()
                        .flatten()
                        .filter_map(|i| effects.get(i))
                        .any(|item| badge(item) == 600160);
                    if adjacent {
                        return confirm(
                            "fuse.guard.kaviPrestartExpansion.title",
                            "fuse.guard.kaviPrestartExpansion.message",
                            json!({"expBadgeId":600160}),
                        );
                    }
                }
            }
        }
        if packet.method == ".lq.Lobby.amuletActivityEndShopping" {
            if enabled(&config, "enable_exit_coin_guard", true)
                && effects.iter().any(|item| base(item) == 227)
                && effects.iter().any(|item| base(item) == 207)
                && number(state.get("coin")) != 0
            {
                return confirm(
                    "fuse.guard.exitCoin.title",
                    "fuse.guard.exitCoin.message",
                    json!({"coin":number(state.get("coin")),"moonId":227,"vacuumId":207}),
                );
            }
            if enabled(&config, "enable_exit_life_guard", false)
                && !effects.iter().any(|item| badge(item) == 600100)
            {
                return confirm(
                    "fuse.guard.noLife.title",
                    "fuse.guard.noLife.message",
                    json!({"lifeBadgeId":600100,"amulets":effects}),
                );
            }
        }
        ModuleAction::Forward
    }
}

pub(super) fn enabled(config: &Value, key: &str, default: bool) -> bool {
    config.get(key).and_then(Value::as_bool).unwrap_or(default)
}
pub(super) fn ints(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}
fn base(item: &Value) -> u64 {
    item.get("id").and_then(Value::as_u64).unwrap_or(0) / 10
}
fn badge(item: &Value) -> u64 {
    item.get("badge")
        .and_then(|v| v.get("id"))
        .or_else(|| item.get("badgeId"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}
fn theft_like(item: &Value) -> bool {
    match base(item) {
        229 => true,
        228 | 232 => item
            .get("store")
            .and_then(Value::as_array)
            .and_then(|store| store.first())
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .is_some_and(|source| source / 10 == 229),
        _ => false,
    }
}
fn number(value: Option<&Value>) -> u64 {
    value
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(0)
}
fn join(values: &[u64]) -> String {
    values
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
