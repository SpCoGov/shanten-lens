//! Optional connection to the local Qyzz client; uses the existing UI state and solvers.
use super::State;
use crate::{pipeline::{Direction, Packet}, services::Services};
use hudsucker::{futures::{SinkExt, StreamExt}, tokio_tungstenite::{connect_async_with_config, tungstenite::{Message, protocol::WebSocketConfig}}};
use serde_json::{json, Value};
use std::{collections::HashMap, path::PathBuf, sync::Mutex, time::Duration};
use tokio::sync::{mpsc, oneshot};

struct Operation {
    packet: Value,
    reply: oneshot::Sender<Result<(), String>>,
}

#[derive(Default)]
pub(super) struct QyzzLink {
    sender: Mutex<Option<mpsc::Sender<Operation>>>,
}

pub(super) fn same_operation_state(before: &Value, after: &Value) -> bool {
    if before["source"] != "qyzz" || after["source"] != "qyzz" { return false; }
    let normalize = |game: &Value| {
        let mut game = game.clone();
        for key in ["revision", "qyzz_version", "update_reason", "action_report", "decision_report"] {
            game.as_object_mut().unwrap().remove(key);
        }
        if let Some(effects) = game["effect_list"].as_array_mut() {
            effects.sort_by_key(|effect| effect["uid"].as_u64());
        }
        game
    };
    normalize(before) == normalize(after)
}

impl QyzzLink {
    pub(super) async fn operate(&self, services: &Services, game: &Value, kind: u64, tiles: &[u64]) -> Result<(), String> {
        let sender = self.sender.lock().unwrap().clone().ok_or("qyzz-disconnected")?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut current = services.game_state();
        if !same_operation_state(game, &current) { return Err("execution-state-mismatch".into()); }
        loop {
            if tokio::time::Instant::now() >= deadline { return Err("qyzz-operation-timeout".into()); }
            let (reply, receive) = oneshot::channel();
            let packet = json!({"type":"operation", "run_id":current["qyzz_run_id"],
                "version":current["qyzz_version"], "operation":kind, "tiles":tiles});
            sender.try_send(Operation { packet, reply }).map_err(|_| "qyzz-busy")?;
            let result = tokio::time::timeout_at(deadline, receive).await
                .map_err(|_| "qyzz-operation-timeout".to_owned())?
                .map_err(|_| "qyzz-disconnected".to_owned())?;
            if result.is_ok() { return result; }
            // Retry only an explicit rejection after a reorder; missing replies may have applied.
            tokio::time::sleep(Duration::from_millis(80)).await;
            let next = services.game_state();
            if next["qyzz_version"].as_u64() <= current["qyzz_version"].as_u64()
                || !same_operation_state(game, &next) { return result; }
            current = next;
        }
    }
}

fn discovery_path(os: &str, env: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    let root = PathBuf::from(env(if os == "macos" { "HOME" } else { "LOCALAPPDATA" })?);
    if !root.is_absolute() { return None; }
    let root = if os == "macos" { root.join("Library/Application Support") } else { root };
    Some(root.join("Qyzz/shanten-lens.json"))
}

fn discovery() -> Option<(u16, String)> {
    let path = discovery_path(std::env::consts::OS, |name| std::env::var_os(name))?;
    #[cfg(test)]
    let path = std::env::var_os("QYZZ_TEST_DISCOVERY").map(PathBuf::from).unwrap_or(path);
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() > 4096 { return None; }
    let data: Value = serde_json::from_slice(&bytes).ok()?;
    if data["protocol"].as_u64()? != 1 { return None; }
    let port = u16::try_from(data["port"].as_u64()?).ok().filter(|port| *port > 0)?;
    let token = data["token"].as_str()?;
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) { return None; }
    Some((port, token.to_owned()))
}

const MAX_PACKET_BYTES: usize = 128 * 1024 * 1024;

fn redact(value: &mut Value) {
    match value {
        Value::Object(fields) => for (key, value) in fields {
            if matches!(key.as_str(), "secret" | "token" | "credential_hash") { *value = json!("[已隐藏]"); }
            else { redact(value); }
        },
        Value::Array(items) => for value in items { redact(value); },
        _ => {}
    }
}

fn record_packet(services: &Services, direction: Direction, data: &Value) -> Result<(), &'static str> {
    let kind = data["type"].as_str().ok_or("missing packet type")?;
    if matches!(kind, "ping" | "pong") { return Ok(()); }
    let mut packet = if kind == "packet" {
        let action = data["action"].as_str().filter(|name| !name.is_empty() && name.len() <= 128).ok_or("invalid packet action")?;
        let direction = match data["direction"].as_str() {
            Some("outbound") => Direction::Outbound,
            Some("inbound") => Direction::Inbound,
            _ => return Err("invalid packet direction"),
        };
        if !data["data"].is_object() { return Err("invalid packet data"); }
        let id = data["data"]["request_id"].as_u64().and_then(|id| u32::try_from(id).ok());
        Packet { direction, packet_type: if direction == Direction::Outbound { "Req" } else if id.is_some() || data["data"]["compressed_reply"].is_object() { "Res" } else { "Notify" }.into(),
            method: format!("qyzz.game.{action}"), id, data: data["data"].clone() }
    } else {
        let packet_type = match kind { "auth" | "operation" | "ping" => "Req", "authenticated" | "result" | "pong" => "Res", _ => "Notify" };
        Packet { direction, packet_type: packet_type.into(), method: format!("qyzz.bridge.{kind}"),
            id: data["id"].as_u64().and_then(|id| u32::try_from(id).ok()), data: data.clone() }
    };
    redact(&mut packet.data);
    // Debug records never enter the game pipeline or get forwarded back to Qyzz.
    services.record_received_packet(&packet);
    services.record_packet(&packet);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_state_allows_only_reordering_and_metadata() {
        let game = json!({"source":"qyzz","session_id":1,"qyzz_run_id":"run","qyzz_deal_id":1,
            "revision":1,"qyzz_version":1,"hand_tiles":[1,2],"wall_tiles":[3,4],
            "replacement_tiles":[5,6],"locked_tiles":[],"ming":[],"stage":4,"ended":false,
            "next_operation":[{"type":101}],"total_change_tile_count":2,
            "effect_list":[{"id":10,"uid":1,"store":["0"]},{"id":20,"uid":2,"store":[]}]});
        let mut reordered = game.clone();
        reordered["effect_list"].as_array_mut().unwrap().reverse();
        reordered["revision"] = json!(2);
        reordered["qyzz_version"] = json!(2);
        for key in ["update_reason", "action_report", "decision_report"] { reordered[key] = json!({"updated":true}); }
        assert!(same_operation_state(&game, &reordered));
        for (field, changed) in [
            ("source",json!("packet")), ("session_id",json!(2)), ("qyzz_run_id",json!("new-run")),
            ("qyzz_deal_id",json!(2)), ("hand_tiles",json!([1,3])), ("wall_tiles",json!([4,3])),
            ("replacement_tiles",json!([6,5])), ("locked_tiles",json!([1])), ("ming",json!([{"tileList":[1,2]}])),
            ("stage",json!(5)), ("ended",json!(true)), ("next_operation",json!([])),
            ("total_change_tile_count",json!(1)), ("new_rule",json!(true)),
        ] {
            let mut changed_game = reordered.clone();
            changed_game[field] = changed;
            assert!(!same_operation_state(&game, &changed_game), "ignored {field}");
        }
        reordered["effect_list"][0]["store"] = json!(["1"]);
        assert!(!same_operation_state(&game, &reordered));
        reordered["effect_list"] = json!([]);
        assert!(!same_operation_state(&game, &reordered));
    }

    #[tokio::test]
    async fn reorders_retry_rejections_but_never_replay_unconfirmed_operations() {
        let root = std::env::temp_dir().join(format!("lens-reorder-{}", std::process::id()));
        let config = root.join("configs");
        let services = tokio::task::spawn_blocking(move || {
            let (events, _) = tokio::sync::broadcast::channel(32);
            Services::load(config, events).unwrap()
        }).await.unwrap();
        let link = QyzzLink::default();
        let (sender, mut receiver) = mpsc::channel::<Operation>(4);
        *link.sender.lock().unwrap() = Some(sender);
        for case in ["before-send", "rejected", "late-state", "accepted", "hand-changed", "lost-reply", "ordinary-error"] {
            services.publish_qyzz_state(&json!({"source":"qyzz","qyzz_run_id":case,"qyzz_deal_id":1,"qyzz_version":1,
                "stage":4,"deck_map":{"1":"1m","2":"2m","3":"3m"},"hand_tiles":[1,2],"wall_tiles":[3],
                "replacement_tiles":[],"locked_tiles":[],"dora_tiles":[],"next_operation":[{"type":101}],
                "effect_list":[{"id":10,"uid":1,"store":[]},{"id":20,"uid":2,"store":[]}]})).unwrap();
            let game = services.game_state();
            let mut reordered = game.clone();
            reordered["effect_list"].as_array_mut().unwrap().reverse();
            reordered["qyzz_version"] = json!(2);
            if case == "before-send" { services.publish_qyzz_state(&reordered).unwrap(); }
            let respond = async {
                let command = receiver.recv().await.unwrap();
                assert_eq!(command.packet["operation"], 101);
                assert_eq!(command.packet["tiles"], json!([1]));
                assert_eq!(command.packet["version"], if case == "before-send" {2} else {1});
                if case == "before-send" { command.reply.send(Ok(())).unwrap(); return; }
                if case == "late-state" {
                    command.reply.send(Err("version-conflict".into())).unwrap();
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    services.publish_qyzz_state(&reordered).unwrap();
                } else {
                    if case == "hand-changed" { reordered["hand_tiles"] = json!([2,3]); }
                    if case != "ordinary-error" { services.publish_qyzz_state(&reordered).unwrap(); }
                    if case == "lost-reply" { drop(command.reply); return; }
                    command.reply.send(if case == "accepted" {Ok(())} else {Err("version-conflict".into())}).unwrap();
                }
                if matches!(case, "rejected" | "late-state") {
                    let retry = tokio::time::timeout(Duration::from_secs(1), receiver.recv()).await.unwrap().unwrap();
                    assert_eq!(retry.packet["version"], 2);
                    assert_eq!(retry.packet["tiles"], json!([1]));
                    retry.reply.send(Ok(())).unwrap();
                }
            };
            let (result, ()) = tokio::join!(link.operate(&services, &game, 101, &[1]), respond);
            match case {
                "hand-changed" | "ordinary-error" => assert_eq!(result.unwrap_err(), "version-conflict"),
                "lost-reply" => assert_eq!(result.unwrap_err(), "qyzz-disconnected"),
                _ => result.unwrap(),
            }
            assert!(receiver.try_recv().is_err(), "unexpected duplicate operation: {case}");
        }
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovery_paths_match_the_game_on_mac_and_windows() {
        let home = std::env::temp_dir().join("Qyzz Test User");
        let env = |key: &str| (key == "HOME").then(|| home.clone().into_os_string());
        assert_eq!(discovery_path("macos", env), Some(home.join("Library/Application Support/Qyzz/shanten-lens.json")));
        assert_eq!(discovery_path("windows", env), None);
        let local = std::env::temp_dir().join("Qyzz AppData Local");
        let env = |key: &str| (key == "LOCALAPPDATA").then(|| local.clone().into_os_string());
        assert_eq!(discovery_path("windows", env), Some(local.join("Qyzz/shanten-lens.json")));
        assert_eq!(discovery_path("macos", env), None);
        for os in ["windows", "macos"] {
            assert_eq!(discovery_path(os, |_| None), None);
            assert_eq!(discovery_path(os, |_| Some("".into())), None);
            assert_eq!(discovery_path(os, |_| Some("relative".into())), None);
        }
    }

    #[test]
    fn bridge_and_game_packets_share_diagnostics_without_replay_or_credentials() {
        let root = std::env::temp_dir().join(format!("qyzz-packet-test-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let (events, _) = tokio::sync::broadcast::channel(32);
        let services = Services::load(root.join("configs"), events).unwrap();
        let before = services.game_state();
        services.set_packet_recording(true).unwrap();
        let auth = json!({"type":"auth", "token":"bridge-secret", "packet_log":true});
        record_packet(&services, Direction::Outbound, &auth).unwrap();
        for (kind, direction) in [("authenticated", Direction::Inbound), ("ping", Direction::Outbound), ("pong", Direction::Inbound), ("operation", Direction::Outbound), ("result", Direction::Inbound), ("state", Direction::Inbound)] {
            record_packet(&services, direction, &json!({"type":kind,"id":42,"ok":false,"error":"test"})).unwrap();
        }
        let request = json!({"type":"packet","direction":"outbound","action":"authenticate","data":{"request_id":7,"args":{"secret":"account-secret","nested":[{"token":"nested-secret","credential_hash":"hash-secret"}]}}});
        record_packet(&services, Direction::Inbound, &request).unwrap();
        let padding = "x".repeat(2 * 1024 * 1024 + 1);
        record_packet(&services, Direction::Inbound, &json!({"type":"packet","direction":"inbound","action":"status","data":{"request_id":7,"result":{"ok":true,"padding":padding}}})).unwrap();
        record_packet(&services, Direction::Inbound, &json!({"type":"packet","direction":"inbound","action":"server_message","data":{"request_id":-1,"result":{"server_message":{"text":"notice"}}}})).unwrap();
        for invalid in [json!({"type":"packet","direction":"sideways","action":"status","data":{}}), json!({"type":"packet","direction":"inbound","action":"status","data":[]})] {
            assert!(record_packet(&services, Direction::Inbound, &invalid).is_err());
        }
        let packets = services.packet_log_snapshot().packets;
        assert_eq!(packets.len(), 8);
        assert!(!packets.iter().any(|packet| matches!(packet.method.as_str(), "qyzz.bridge.ping" | "qyzz.bridge.pong")));
        assert_eq!(packets[0].data["token"], "[已隐藏]");
        assert_eq!(auth["token"], "bridge-secret");
        assert_eq!(request["data"]["args"]["secret"], "account-secret");
        assert_eq!(packets[5].method, "qyzz.game.authenticate");
        assert_eq!(packets[5].direction, Direction::Outbound);
        assert_eq!(packets[5].id, Some(7));
        assert_eq!(packets[5].data["args"]["nested"][0]["token"], "[已隐藏]");
        assert_eq!(packets[6].data["result"]["padding"], padding);
        assert_eq!(packets[7].packet_type, "Notify");
        let delta = json!({"request_id":8,"result":{"state_delta":true,"base_version":4,"version":5,"view":{"hand":{"dirty":true,"value":[17,2]}},"events":[]}});
        record_packet(&services, Direction::Inbound, &json!({"type":"packet","direction":"inbound","action":"exchange","data":delta})).unwrap();
        let compressed = json!({"compressed_reply":{"encoding":"base64","bytes":3,"data":"AID/"}});
        record_packet(&services, Direction::Inbound, &json!({"type":"packet","direction":"inbound","action":"compressed_reply","data":compressed})).unwrap();
        let chunk = json!({"request_id":9,"offset":0,"total":3,"reply_chunk":{"encoding":"base64","bytes":3,"data":"AID/"}});
        record_packet(&services, Direction::Inbound, &json!({"type":"packet","direction":"inbound","action":"exchange","data":chunk})).unwrap();
        let raw = services.packet_log_snapshot().packets;
        assert_eq!(raw[8].data, delta);
        assert_eq!(raw[9].data, compressed);
        assert_eq!(raw[9].packet_type, "Res");
        assert_eq!(raw[10].data, chunk);
        assert_eq!(raw[10].packet_type, "Res");
        assert!(packets.iter().all(|packet| !services.can_replay(&packet.method)));
        assert_eq!(services.game_state(), before);
        let recording = services.set_packet_recording(false).unwrap();
        assert_eq!(recording.count, 11);
        let recorded = std::fs::read_to_string(recording.path.unwrap()).unwrap();
        for secret in ["bridge-secret", "account-secret", "nested-secret", "hash-secret"] { assert!(!recorded.contains(secret)); }
        drop(services);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn custom_plans_execute_exchanges_and_zero_to_four_quads() {
        const CHILD_ROOT: &str = "SHANTEN_CUSTOM_EXECUTION_TEST_ROOT";
        let root = match std::env::var_os(CHILD_ROOT) {
            Some(root) => PathBuf::from(root),
            None => {
                let root = std::env::temp_dir().join(format!("lens-custom-execution-{}", std::process::id()));
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "embedded::qyzz::tests::custom_plans_execute_exchanges_and_zero_to_four_quads", "--nocapture"])
                    .env(CHILD_ROOT, &root).output().unwrap();
                assert!(output.status.success(), "{output:?}");
                std::fs::remove_dir_all(root).unwrap();
                return;
            }
        };
        let runtime = tokio::task::spawn_blocking(move || super::super::BackendRuntime::load(root.join("configs")))
            .await.unwrap().unwrap();
        let state = &runtime.state;
        let (sender, mut receiver) = mpsc::channel::<Operation>(8);
        *state.qyzz.sender.lock().unwrap() = Some(sender);
        let services = state.services.clone();
        let game_task = tokio::spawn(async move {
            let mut reordered = std::collections::HashSet::new();
            while let Some(command) = receiver.recv().await {
                let mut game = services.game_state();
                let mut hand = game["hand_tiles"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
                let tiles = command.packet["tiles"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
                assert_eq!(command.packet["version"], game["qyzz_version"]);
                let kind = command.packet["operation"].as_u64().unwrap();
                let timing = game["execution_test_timing"].as_str().unwrap_or("").to_owned();
                if timing.starts_with("reorder") && reordered.insert(format!("{}-{kind}-{tiles:?}", game["qyzz_run_id"])) {
                    game["effect_list"].as_array_mut().unwrap().reverse();
                    game["qyzz_version"] = json!(game["qyzz_version"].as_u64().unwrap() + 1);
                    services.publish_qyzz_state(&game).unwrap();
                    let _ = command.reply.send(Err("stale-version".into()));
                    continue;
                }
                let exchange_stage = if timing.starts_with("packet-") { 5 } else { 4 };
                let play_stage = exchange_stage + 1;
                let previous_melds = game["ming"].clone();
                let draw = match kind {
                    101 => {
                        assert!(tiles.iter().all(|id| hand.contains(id)));
                        let count = hand.len() - tiles.len();
                        let pool = game["replacement_tiles"].as_array_mut().unwrap();
                        let incoming = pool.drain(..count).map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
                        hand = tiles;
                        hand.extend(incoming);
                        game["total_change_tile_count"] = json!(game["total_change_tile_count"].as_u64().unwrap() - 1);
                        if timing.ends_with("auto-finish") && game["total_change_tile_count"] == 0 {
                            game["stage"] = json!(play_stage);
                            true
                        } else { false }
                    }
                    100 => { game["stage"] = json!(play_stage); hand.len() == 13 }
                    1 | 4 => {
                        assert!(tiles.iter().all(|id| hand.contains(id) && game["deck_map"][id.to_string()] != "bd"));
                        hand.retain(|id| !tiles.contains(id));
                        if command.packet["operation"] == 4 {
                            game["ming"].as_array_mut().unwrap().push(json!({"type":4, "tileList":tiles}));
                        }
                        true
                    }
                    kind => panic!("unexpected operation {kind}"),
                };
                if draw && !game["wall_tiles"].as_array().unwrap().is_empty() {
                    let tile = game["wall_tiles"].as_array_mut().unwrap().remove(0).as_u64().unwrap();
                    hand.push(tile);
                }
                game["hand_tiles"] = json!(hand);
                game["next_operation"] = if game["stage"] == exchange_stage {json!([{"type":101},{"type":100}])} else {json!([{"type":1},{"type":4}])};
                let committed = game.clone();
                if kind == 4 && !timing.is_empty() { game["ming"] = previous_melds; }
                if kind == 100 && !timing.is_empty() { game["next_operation"] = json!([{"type":1}]); }
                game["qyzz_version"] = json!(game["qyzz_version"].as_u64().unwrap() + 1);
                services.publish_qyzz_state(&game).unwrap();
                let _ = command.reply.send(Ok(()));
                if !timing.is_empty() && matches!(kind, 4 | 100) {
                    tokio::time::sleep(Duration::from_millis(160)).await;
                    assert!(receiver.try_recv().is_err(), "An operation was sent before the kan/state was confirmed: {game}");
                    if timing == "missing-kan" && kind == 4 { continue; }
                    game["ming"] = committed["ming"].clone();
                    game["next_operation"] = committed["next_operation"].clone();
                    game["qyzz_version"] = json!(game["qyzz_version"].as_u64().unwrap() + 1);
                    services.publish_qyzz_state(&game).unwrap();
                }
            }
        });
        for (quads, joker) in [(0, false), (1, false), (2, false), (3, false), (4, false), (1, true)] {
            let mut faces = (1..=quads).flat_map(|rank| vec![format!("{rank}m"); 4]).collect::<Vec<_>>();
            faces.extend(vec!["7z".to_owned(); 14 - quads * 3]);
            if joker { faces[quads * 4] = "bd".into(); }
            let replacements = if quads == 0 { 4 } else { 1 };
            let wall = std::iter::once("6z".to_owned()).chain(faces.iter().skip(13).cloned()).collect::<Vec<_>>();
            let deck = (1..=13).zip(faces.iter().take(13)).map(|(id, face)| (id.to_string(), json!(if id<=replacements {"9s"} else {face})))
                .chain(faces.iter().take(replacements).enumerate().map(|(i, face)| ((14+i).to_string(), json!(face))))
                .chain(wall.iter().enumerate().map(|(i, face)| ((14+replacements+i).to_string(), json!(face))))
                .collect::<serde_json::Map<_, _>>();
            let game = json!({"source":"qyzz", "qyzz_run_id":format!("custom-{quads}-{joker}"), "qyzz_version":1, "qyzz_deal_id":1,
                "stage":4, "deck_map":deck, "hand_tiles":(1..=13).collect::<Vec<_>>(), "replacement_tiles":(14..14+replacements).collect::<Vec<_>>(),
                "wall_tiles":(14+replacements..16+replacements+quads).collect::<Vec<_>>(), "locked_tiles":[], "dora_tiles":[], "effect_list":[],
                "ming":[], "boss_buff":[901], "total_change_tile_count":2, "next_operation":[{"type":101},{"type":100}]});
            state.services.publish_qyzz_state(&game).unwrap();
            let mut target = (1..=quads).map(|rank| json!({"quad":true,"rule":{"faces":[format!("{rank}m")]}})).collect::<Vec<_>>();
            target.extend(faces.iter().skip(quads*4).map(|face| json!({"rule":{"faces":[if face=="bd" {"1z"} else {face}],
                "joker":if face=="bd" {"allow"} else {"exclude"}}})));
            let replies = super::super::switch::switch_command(state, &json!({"action":"start",
                "options":{"search_algorithm":"custom_target","target_groups":target,"wall_limit":36}})).await;
            let plan = &replies[0]["data"][0]["data"];
            assert_eq!(plan["status"], "plan", "{plan}");
            assert!(plan["custom_steps"].as_array().unwrap().iter().any(|step| step["kind"] == "discard"));
            if quads == 0 { assert_eq!(plan["switch_discards"].as_array().unwrap().len(), 2); }
            let exchanged = super::super::switch::switch_command(state, &json!({"action":"execute_plan","options":{"plan_id":plan["plan_id"]}})).await;
            assert_eq!(exchanged[0]["data"]["ok"], true, "{exchanged:?}");
            assert_eq!(state.services.game_state()["stage"], 4);
            assert_eq!(state.services.game_state()["ming"], json!([]));
            assert_eq!(state.services.game_state()["wall_tiles"], game["wall_tiles"]);
            let replies = super::super::switch::switch_command(state, &json!({"action":"start",
                "options":{"search_algorithm":"custom_target","target_groups":target,"wall_limit":36}})).await;
            let plan = &replies[0]["data"][0]["data"];
            assert_eq!(plan["status"], "plan", "{plan}");
            let mut execution_events = state.events.subscribe();
            let result = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":plan["plan_id"]}})).await;
            let mut execution_count = 0;
            while let Ok(event) = execution_events.try_recv() {
                if event["type"] == "souzu_switch_execution" {
                    assert_eq!(event["data"]["search_algorithm"], plan["search_algorithm"]);
                    assert_eq!(event["data"]["plan_id"], plan["plan_id"]);
                    execution_count += 1;
                }
            }
            assert!(execution_count > 0);
            assert_eq!(result[0]["data"]["ok"], true, "{result:?}");
            let game = state.services.game_state();
            assert_eq!(game["ming"].as_array().unwrap().len(), quads);
            assert!(game["hand_tiles"].as_array().unwrap().contains(&json!(14)) || game["ming"].as_array().unwrap().iter()
                .any(|meld| meld["tileList"].as_array().unwrap().contains(&json!(14))));
            let stale = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":plan["plan_id"]}})).await;
            assert_eq!(stale[0]["data"]["reason"], "stale-plan");
            if quads == 0 {
                let replies = super::super::switch::switch_command(state, &json!({"action":"start",
                    "options":{"search_algorithm":"custom_target","target_groups":target,"wall_limit":36}})).await;
                let matched = &replies[0]["data"][0]["data"];
                assert_eq!(matched["custom_steps"], json!([]));
                let result = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":matched["plan_id"]}})).await;
                assert_eq!(result[0]["data"]["ok"], true, "{result:?}");
                assert_eq!(state.services.game_state(), game);
            }
        }
        for (objective, timing) in [("heavenly_blessing", ""), ("shining_pearl", "packet-auto-finish")] {
            let faces = ["1m", "2m", "3m", "1s", "2s", "3s", "7p", "8p", "9p", "4p", "5p", "7z", "6p", "7z", "0p", "7z"];
            let mut deck = faces.iter().enumerate().map(|(i, face)| ((i + 1).to_string(), json!(face)))
                .collect::<serde_json::Map<_, _>>();
            deck.insert("1000".into(), json!("bd"));
            deck.insert("500".into(), json!("4p"));
            let game = json!({"source":"qyzz","qyzz_run_id":format!("wanxiang-{objective}-{timing}"),"qyzz_version":1,"qyzz_deal_id":1,
                "stage":if timing.starts_with("packet-") {5} else {4},"deck_map":deck,"hand_tiles":(1..=12).chain([1000]).collect::<Vec<_>>(),
                "replacement_tiles":[13],"wall_tiles":[14,15,16],"locked_tiles":[],"dora_tiles":[500],"tian_dora_tiles":["5p"],
                "effect_list":[],"ming":[],"total_change_tile_count":1,"next_operation":[{"type":101},{"type":100}],"execution_test_timing":timing});
            state.services.publish_qyzz_state(&game).unwrap();
            let replies = super::super::switch::switch_command(state, &json!({"action":"start",
                "options":{"search_algorithm":"wanxiang_four_meld_switch","wall_limit":36}})).await;
            let ordinary = &replies[0]["data"][0]["data"];
            assert_eq!(ordinary["status"], "plan", "{ordinary}");
            let before = state.services.game_state();
            let rejected = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":ordinary["plan_id"]}})).await;
            assert_eq!(rejected[0]["data"]["reason"], "wanxiang-plan-has-no-full-kan-execution");
            assert_eq!(state.services.game_state(), before);
            let replies = super::super::switch::switch_command(state, &json!({"action":"start",
                "options":{"search_algorithm":"wanxiang_four_meld_switch","wall_limit":36,"preferences":{"coin_objective":objective}}})).await;
            let plan = &replies[0]["data"][0]["data"];
            assert_eq!(plan["status"], "plan", "{plan}");
            assert!(plan["custom_steps"].as_array().unwrap().iter().any(|step| step["kind"] == "discard" && step["tiles"] == json!([14])));
            let result = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":plan["plan_id"]}})).await;
            assert_eq!(result[0]["data"]["ok"], true, "{result:?}");
            let game = state.services.game_state();
            let mut actual = game["hand_tiles"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
            actual.sort_unstable();
            let mut target = plan["target_physical_ids"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
            target.sort_unstable();
            assert_eq!(actual, target);
            assert!(actual.contains(&11) && actual.contains(&15) && actual.contains(&1000));
            assert_eq!(game["wall_tiles"], json!([16]));
            assert_eq!(game["ming"], json!([]));
        }
        for (custom, quads, timing) in [(true, 2, "packet-auto-finish"), (true, 2, "packet-delayed"), (false, 2, "packet-auto-finish"),
            (true, 2, "delayed"), (false, 2, "delayed"), (false, 3, "delayed"),
            (true, 2, "auto-finish"), (false, 2, "auto-finish"),
            (true, 2, "reorder"), (false, 2, "reorder"),
            (true, 2, "reorder-auto-finish"), (false, 2, "reorder-auto-finish"),
            (false, 2, "empty-wall"),
            (true, 2, "missing-kan"), (false, 2, "missing-kan")] {
            let mut hand = vec!["1m"; 4];
            hand.extend(["5p", "5p", "0p", "5p"]);
            if quads == 3 { hand.extend(["9s"; 4]); hand.push("5s"); }
            else { hand.extend(["3s", "3s", "3s", "4s", "5s"]); }
            let wall = if timing == "empty-wall" { vec!["6z", "4s", "6s"] }
                else if quads == 3 { vec!["6z", "5s", "6s", "7s", "8s"] }
                else { vec!["6z", "4s", "6s", "7s", "8s"] };
            let mut deck = hand.iter().enumerate().map(|(i, face)| ((i+1).to_string(), json!(face)))
                .chain(wall.iter().enumerate().map(|(i, face)| ((14+i).to_string(), json!(face))))
                .collect::<serde_json::Map<_, _>>();
            if timing.ends_with("auto-finish") {
                deck.insert("1".into(), json!("9m"));
                deck.insert("19".into(), json!("1m"));
            }
            let mut game = json!({"source":"qyzz","qyzz_run_id":format!("timing-{custom}-{quads}-{timing}"),"qyzz_version":1,"qyzz_deal_id":1,
                "stage":if timing.starts_with("packet-") {5} else {4},"deck_map":deck,"hand_tiles":(1..=13).collect::<Vec<_>>(),"wall_tiles":(14..14+wall.len()).collect::<Vec<_>>(),
                "replacement_tiles":if timing.ends_with("auto-finish") {json!([19])} else {json!([])},
                "locked_tiles":[],"dora_tiles":[],"effect_list":[],"ming":[],"total_change_tile_count":u8::from(timing.ends_with("auto-finish")),
                "next_operation":[{"type":101},{"type":100}],"execution_test_timing":timing});
            if timing.starts_with("reorder") { game["effect_list"] = json!([{"id":10,"uid":1,"store":[]},{"id":20,"uid":2,"store":[]}]); }
            state.services.publish_qyzz_state(&game).unwrap();
            let mut groups = vec![json!({"quad":true,"rule":{"faces":["1m"]}}),json!({"quad":true,"rule":{"faces":["5p"]}})];
            let concealed = ["3s","3s","3s","4s","4s","5s","6s","7s"];
            groups.extend(concealed.map(|face| json!({"rule":{"faces":[face],"joker":"exclude"}})));
            let options = if custom {json!({"search_algorithm":"custom_target","target_groups":groups,"wall_limit":36})}
                else {json!({"search_algorithm":if quads==3 {"target_enumeration_three_quads"} else {"target_enumeration_search"},"wall_limit":36})};
            let replies = super::super::switch::switch_command(state, &json!({"action":"start","options":options})).await;
            let plan = &replies[0]["data"][0]["data"];
            assert_eq!(plan["status"], "plan", "{plan}");
            let result = super::super::switch::switch_command(state, &json!({"action":"execute_full_plan","options":{"plan_id":plan["plan_id"]}})).await;
            if timing == "missing-kan" {
                assert_eq!(result[0]["data"]["ok"], false, "{result:?}");
                assert_eq!(result[0]["data"]["reason"], "execution-state-mismatch");
                assert_eq!(result[0]["data"]["reason_key"], "blackhole.execute_reason_state_mismatch");
                let game = state.services.game_state();
                assert_eq!(game["ming"], json!([]), "An unconfirmed kan was counted as completed");
                assert_eq!(game["wall_tiles"].as_array().unwrap().len(), 3, "Execution kept discarding after a missing kan");
            } else {
                assert_eq!(result[0]["data"]["ok"], true, "{result:?}");
                assert_eq!(state.services.game_state()["ming"].as_array().unwrap().len(), quads);
            }
        }
        game_task.abort();
    }

    #[tokio::test]
    async fn round_profiles_execute_plan_repeat_wins_and_stop() {
        const CHILD_ROOT: &str = "SHANTEN_ROUND_EXECUTION_TEST_ROOT";
        let root = match std::env::var_os(CHILD_ROOT) {
            Some(root) => PathBuf::from(root),
            None => {
                let root = std::env::temp_dir().join(format!("lens-round-execution-{}", std::process::id()));
                let output = std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "embedded::qyzz::tests::round_profiles_execute_plan_repeat_wins_and_stop", "--nocapture"])
                    .env(CHILD_ROOT, &root).output().unwrap();
                assert!(output.status.success(), "{output:?}");
                std::fs::remove_dir_all(root).unwrap();
                return;
            }
        };
        let runtime = tokio::task::spawn_blocking(move || super::super::BackendRuntime::load(root.join("configs")))
            .await.unwrap().unwrap();
        let state = &runtime.state;
        for profile_id in ["opening-profile", "profile-a"] {
            let mut book: Value = serde_json::from_str(include_str!("../../tests/fixtures/playbook.json")).unwrap();
            book["selected_profile_id"] = json!(profile_id);
            let target = ["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p","5p","5p"];
            book["profiles"][0]["stages"][0]["hands"][0]["target_groups"] = json!(target.map(|face|
                json!({"kind":"single","rule":{"faces":[face]}})));
            state.services.patch_config(&json!({"autorun":{"playbook":book,"op_interval_ms":100}})).unwrap();
            let faces = ["1p","1p","1p","2p","2p","2p","3p","3p","3p","4p","4p","4p","5m","5p","5p","9s","8s"];
            let game = json!({"source":"qyzz","qyzz_run_id":profile_id,"qyzz_version":1,"qyzz_deal_id":1,
                "stage":5,"level":13001,"node":3,"deck_map":faces.iter().enumerate()
                    .map(|(i, face)|((i+1).to_string(),json!(face))).collect::<serde_json::Map<_,_>>(),
                "hand_tiles":(1..=13).collect::<Vec<_>>(),"wall_tiles":[15,16,17],"replacement_tiles":[14],
                "locked_tiles":[],"dora_tiles":[],"effect_list":[],"ming":[],"total_change_tile_count":1,
                "change_tile_count":0,"next_operation":[{"type":101},{"type":100}]});
            state.services.publish_qyzz_state(&game).unwrap();
            let (sender, mut receiver) = mpsc::channel::<Operation>(8);
            *state.qyzz.sender.lock().unwrap() = Some(sender);
            let services = state.services.clone();
            let game_task = tokio::spawn(async move {
                let mut operations = Vec::new();
                let mut wins = 0;
                while let Some(command) = receiver.recv().await {
                    let mut game = services.game_state();
                    assert_eq!(command.packet["version"], game["qyzz_version"]);
                    let kind = command.packet["operation"].as_u64().unwrap();
                    operations.push(kind);
                    match kind {
                        101 => {
                            let mut hand = command.packet["tiles"].as_array().unwrap().clone();
                            assert_eq!(hand.len(), 12);
                            assert!(!hand.contains(&json!(13)));
                            hand.push(game["replacement_tiles"].as_array_mut().unwrap().remove(0));
                            game["hand_tiles"] = json!(hand);
                            game["change_tile_count"] = json!(1);
                            game["next_operation"] = json!([{"type":100}]);
                        },
                        100 => {
                            let tile = game["wall_tiles"].as_array_mut().unwrap().remove(0);
                            game["hand_tiles"].as_array_mut().unwrap().push(tile);
                            game["stage"] = json!(6);
                            game["next_operation"] = json!([{"type":1},{"type":8}]);
                        },
                        8 => {
                            wins += 1;
                            game["wall_tiles"].as_array_mut().unwrap().remove(0);
                            let _ = command.reply.send(Ok(()));
                            tokio::time::sleep(Duration::from_millis(220)).await;
                            assert!(receiver.try_recv().is_err(), "tsumo repeated without a new snapshot");
                            game["qyzz_version"] = json!(game["qyzz_version"].as_u64().unwrap() + 1);
                            services.publish_qyzz_state(&game).unwrap();
                            if wins == 2 {
                                tokio::time::sleep(Duration::from_millis(220)).await;
                                assert!(receiver.try_recv().is_err(), "an operation was sent after Stop");
                                return operations;
                            }
                            continue;
                        },
                        other => panic!("unexpected round operation {other}"),
                    }
                    game["qyzz_version"] = json!(game["qyzz_version"].as_u64().unwrap() + 1);
                    services.publish_qyzz_state(&game).unwrap();
                    let _ = command.reply.send(Ok(()));
                }
                panic!("round runner disconnected before repeated wins");
            });
            let mut updates = state.events.subscribe();
            assert!(runtime.autorun(crate::ipc::AutorunAction::Start, true).await.ok);
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    let event = updates.recv().await.unwrap();
                    assert_ne!(event["type"], "souzu_switch_execution", "automatic round execution must only report progress through the monitor panel");
                    if event["type"] == "autorun_status" && event["data"]["round_decision"]["actual_wins"] == 2 { break; }
                }
            }).await.expect("round runner did not complete two wins");
            assert_eq!(state.automation.autorun_status()["round_decision"]["strategy"],
                if profile_id == "opening-profile" { "opening" } else { "full" });
            assert!(runtime.autorun(crate::ipc::AutorunAction::Stop, false).await.ok);
            assert_eq!(game_task.await.unwrap(), [101,100,8,8]);
            assert_eq!(state.automation.autorun_status()["round_decision"]["status"], "stopped");
        }
    }

    /// Run alongside tests/lens_bridge_test.gd -- --serve-lens-test in the Qyzz checkout.
    #[tokio::test]
    #[ignore = "requires an isolated running Qyzz bridge fixture"]
    async fn real_qyzz_connection_search_and_exchange() {
        let path = PathBuf::from(std::env::var_os("QYZZ_TEST_ROOT").expect("isolated test root"));
        let runtime = tokio::task::spawn_blocking(move || super::super::BackendRuntime::load(path)).await.unwrap().unwrap();
        let state = runtime.state.clone();
        let connection_state = state.clone();
        let task = tokio::spawn(async move { run(&connection_state).await });
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while state.services.game_state()["hand_tiles"].as_array().is_none_or(|hand| hand.len() != 13) {
            assert!(tokio::time::Instant::now() < deadline, "Qyzz connection timed out");
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let game = state.services.game_state();
        assert_eq!(game["source"], "qyzz");
        assert_eq!(game["max_effect_volume"], 10);
        let full_quads = std::env::var("QYZZ_TEST_FULL_PLAN").ok().map(|n| n.parse::<usize>().unwrap());
        let algorithm = if full_quads == Some(3) { "target_enumeration_three_quads" }
            else if game["deck_map"]["1000"] == "bd" { "wanxiang_four_meld_switch" } else { "target_enumeration_search" };
        super::super::switch::switch_command(&state, &json!({"action":"start", "options":{"search_algorithm":algorithm,"wall_limit":36}})).await;
        let plan = state.switch_plan.read().await.clone().unwrap();
        assert_eq!(plan["status"], "plan", "{plan}");
        assert!(plan["switch_discards"].as_array().is_some_and(|batches| !batches.is_empty()));
        println!("Live bridge: {algorithm}={}", plan["status"]);
        let mut keep = game["hand_tiles"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
        keep.remove(keep.iter().position(|id| *id != 1000).unwrap());
        let incoming = game["replacement_tiles"][0].clone();
        let action = if full_quads.is_some() { "execute_full_plan" } else { "execute_plan" };
        let executed = super::super::switch::switch_command(&state, &json!({"action":action, "options":{"plan_id":plan["plan_id"]}})).await;
        assert_eq!(executed[0]["data"]["ok"], true, "{executed:?}");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while state.services.game_state()["qyzz_version"] == game["qyzz_version"] {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let next = state.services.game_state();
        assert!(next["hand_tiles"].as_array().unwrap().contains(&incoming)
            || (full_quads.is_some() && next["ming"].as_array().unwrap().iter()
                .any(|meld| meld["tileList"].as_array().unwrap().contains(&incoming))));
        assert_eq!(next["session_id"], game["session_id"]);
        if let Some(count) = full_quads {
            assert_eq!(next["ming"].as_array().unwrap().len(), count, "Full plan did not declare every quad: {next}");
            assert!(!next["used_desktop_tiles"].as_array().unwrap().is_empty(), "Full plan did not discard after switching and declaring quads");
            println!("Full plan completed: {count} quads and {} discards", next["used_desktop_tiles"].as_array().unwrap().len());
        }
        assert!(state.qyzz.operate(&state.services, &game, 101, &keep).await.is_err(), "stale revision was accepted");
        assert!(state.proxy.request(".lq.Lobby.amuletActivityGameOperate", &json!({}), Duration::from_secs(1)).await.is_err());
        let packets = state.services.packet_log_snapshot().packets;
        for method in ["qyzz.bridge.auth", "qyzz.bridge.authenticated", "qyzz.bridge.state", "qyzz.bridge.operation", "qyzz.bridge.result", "qyzz.game.exchange"] {
            assert!(packets.iter().any(|packet| packet.method == method), "missing live debug traffic: {method}");
        }
        assert!(!packets.iter().any(|packet| matches!(packet.method.as_str(), "qyzz.bridge.ping" | "qyzz.bridge.pong")));
        assert!(!packets.iter().any(|packet| packet.method == "qyzz.game.lens_state"), "state conversion must stay in the game client");
        assert!(packets.iter().any(|packet| packet.method == "qyzz.game.exchange" && packet.direction == Direction::Outbound));
        let exchange = packets.iter().find(|packet| packet.method == "qyzz.game.exchange" && packet.direction == Direction::Inbound).unwrap();
        assert_eq!(exchange.data["result"]["state_delta"], true);
        assert_eq!(exchange.data["result"]["view"]["hand"]["dirty"], true);
        assert!(exchange.data["result"]["view"].get("stage_rows").is_none(), "diagnostics must not expand the wire delta");
        assert_eq!(packets.iter().find(|packet| packet.method == "qyzz.bridge.auth").unwrap().data["token"], "[已隐藏]");
        state.services.disconnect_qyzz();
        assert_eq!(state.services.game_state()["stage"], -1);
        assert_eq!(state.services.game_state()["qyzz_connected"], false);
        task.abort();
    }
}

pub(super) async fn run(state: &State) {
    loop {
        if let Some((port, token)) = discovery() {
            let config = WebSocketConfig::default().max_message_size(Some(MAX_PACKET_BYTES)).max_frame_size(Some(MAX_PACKET_BYTES));
            let connection = tokio::time::timeout(Duration::from_secs(2), connect_async_with_config(format!("ws://127.0.0.1:{port}"), Some(config), false)).await;
            if let Ok(Ok((mut socket, _))) = connection {
                let auth = json!({"type":"auth", "token":token, "packet_log":true});
                if socket.send(Message::Text(auth.to_string().into())).await.is_ok() {
                    let _ = record_packet(&state.services, Direction::Outbound, &auth);
                    let (sender, mut receiver) = mpsc::channel::<Operation>(4);
                    let mut waiting = HashMap::<u64, oneshot::Sender<Result<(), String>>>::new();
                    let mut serial = 0u64;
                    let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
                    let mut last_message = tokio::time::Instant::now();
                    let mut authenticated = false;
                    loop {
                        tokio::select! {
                            message = socket.next() => {
                                let Some(Ok(message)) = message else { break; };
                                last_message = tokio::time::Instant::now();
                                if message.is_close() { break; }
                                let Message::Text(text) = message else { continue; };
                                if text.len() > MAX_PACKET_BYTES { break; }
                                let Ok(data) = serde_json::from_str::<Value>(&text) else { break; };
                                if data["type"] == "packet" && !authenticated { break; }
                                if record_packet(&state.services, Direction::Inbound, &data).is_err() { break; }
                                match data["type"].as_str() {
                                    Some("authenticated") if data["protocol"] == 1 => {
                                        authenticated = true;
                                        *state.qyzz.sender.lock().unwrap() = Some(sender.clone());
                                        tracing::info!("Qyzz local connection established");
                                    }
                                    Some("state") => {
                                        if !authenticated { break; }
                                        if let Err(error) = state.services.publish_qyzz_state(&data["data"]) {
                                            tracing::warn!(%error, "Invalid Qyzz state");
                                            break;
                                        }
                                    }
                                    Some("result") => {
                                        if let Some(reply) = data["id"].as_u64().and_then(|id| waiting.remove(&id)) {
                                            let result = if data["ok"] == true { Ok(()) } else { Err(data["error"].as_str().unwrap_or("qyzz-operation-failed").to_owned()) };
                                            let _ = reply.send(result);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            command = receiver.recv() => {
                                let Some(mut command) = command else { break; };
                                if command.reply.is_closed() { continue; }
                                if !state.services.qyzz_operations_allowed() {
                                    let _ = command.reply.send(Err("data-source-not-selected".into()));
                                    continue;
                                }
                                serial += 1;
                                command.packet["id"] = json!(serial);
                                waiting.insert(serial, command.reply);
                                if socket.send(Message::Text(command.packet.to_string().into())).await.is_err() { break; }
                                let _ = record_packet(&state.services, Direction::Outbound, &command.packet);
                            }
                            _ = heartbeat.tick() => {
                                if last_message.elapsed() > Duration::from_secs(15) { break; }
                                let ping = json!({"type":"ping"});
                                if socket.send(Message::Text(ping.to_string().into())).await.is_err() { break; }
                                let _ = record_packet(&state.services, Direction::Outbound, &ping);
                            }
                        }
                    }
                }
                *state.qyzz.sender.lock().unwrap() = None;
                state.services.disconnect_qyzz();
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
