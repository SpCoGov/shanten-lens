//! Optional connection to the local Qyzz client; uses the existing UI state and solvers.
use super::State;
use hudsucker::{futures::{SinkExt, StreamExt}, tokio_tungstenite::{connect_async, tungstenite::Message}};
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

impl QyzzLink {
    pub(super) async fn operate(&self, game: &Value, kind: u64, tiles: &[u64]) -> Result<(), String> {
        let sender = self.sender.lock().unwrap().clone().ok_or("qyzz-disconnected")?;
        let (reply, receive) = oneshot::channel();
        let packet = json!({"type":"operation", "run_id":game["qyzz_run_id"],
            "version":game["qyzz_version"], "operation":kind, "tiles":tiles});
        sender.try_send(Operation { packet, reply }).map_err(|_| "qyzz-busy")?;
        tokio::time::timeout(Duration::from_secs(20), receive).await
            .map_err(|_| "qyzz-operation-timeout".to_owned())?
            .map_err(|_| "qyzz-disconnected".to_owned())?
    }
}

fn discovery() -> Option<(u16, String)> {
    let path = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("Qyzz/shanten-lens.json");
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let algorithm = if game["deck_map"]["1000"] == "bd" { "wanxiang_four_meld_switch" } else { "target_enumeration_search" };
        super::super::switch::switch_command(&state, &json!({"action":"start", "options":{"search_algorithm":algorithm}})).await;
        let plan = state.switch_plan.read().await.clone().unwrap();
        assert_eq!(plan["status"], "plan", "{plan}");
        assert!(plan["switch_discards"].as_array().is_some_and(|batches| !batches.is_empty()));
        println!("Live bridge: {algorithm}={}", plan["status"]);
        let mut keep = game["hand_tiles"].as_array().unwrap().iter().map(|id| id.as_u64().unwrap()).collect::<Vec<_>>();
        keep.remove(keep.iter().position(|id| *id != 1000).unwrap());
        let incoming = game["replacement_tiles"][0].clone();
        let executed = super::super::switch::switch_command(&state, &json!({"action":"execute_plan", "options":{"plan_id":plan["plan_id"]}})).await;
        assert_eq!(executed[0]["data"]["ok"], true, "{executed:?}");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while state.services.game_state()["qyzz_version"] == game["qyzz_version"] {
            assert!(tokio::time::Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        let next = state.services.game_state();
        assert!(next["hand_tiles"].as_array().unwrap().contains(&incoming));
        assert_eq!(next["session_id"], game["session_id"]);
        assert!(state.qyzz.operate(&game, 101, &keep).await.is_err(), "stale revision was accepted");
        assert!(state.proxy.request(".lq.Lobby.amuletActivityGameOperate", &json!({}), Duration::from_secs(1)).await.is_err());
        state.services.disconnect_qyzz();
        assert_eq!(state.services.game_state()["stage"], -1);
        assert_eq!(state.services.game_state()["qyzz_connected"], false);
        task.abort();
    }
}

pub(super) async fn run(state: &State) {
    loop {
        if let Some((port, token)) = discovery() {
            let connection = tokio::time::timeout(Duration::from_secs(2), connect_async(format!("ws://127.0.0.1:{port}"))).await;
            if let Ok(Ok((mut socket, _))) = connection {
                if socket.send(Message::Text(json!({"type":"auth", "token":token}).to_string().into())).await.is_ok() {
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
                                if text.len() > 2 * 1024 * 1024 { break; }
                                let Ok(data) = serde_json::from_str::<Value>(&text) else { break; };
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
                            }
                            _ = heartbeat.tick() => {
                                if last_message.elapsed() > Duration::from_secs(15) { break; }
                                if socket.send(Message::Text(json!({"type":"ping"}).to_string().into())).await.is_err() { break; }
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
