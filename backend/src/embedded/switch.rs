use super::{BackendRuntime, State};
use crate::{ipc::SwitchRequest, recommendations, services::event};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

impl BackendRuntime {
    pub async fn switch(&self, request: SwitchRequest) {
        let data = json!({
            "action": request.action.as_str(),
            "options": request.options,
            "snapshot": request.snapshot,
            "quad_groups": request.quad_groups,
            "structure_groups": request.structure_groups,
            "notify": request.notify,
        });
        let replies = switch_command(&self.state, &data).await;
        for reply in replies {
            let _ = self.state.events.send(reply);
        }
    }
}

pub(super) async fn switch_command(state: &State, data: &Value) -> Vec<Value> {
    let action = data.get("action").and_then(Value::as_str).unwrap_or("");
    let wall_limit = data
        .get("options")
        .and_then(|v| v.get("wall_limit"))
        .and_then(Value::as_u64)
        .unwrap_or(36) as usize;
    match action {
        "start" | "start_debug" => {
            let preferences = match serde_json::from_value::<recommendations::SearchPreferences>(
                data.pointer("/options/preferences").cloned().unwrap_or_else(|| json!({})),
            ) {
                Ok(preferences) => preferences,
                Err(_) => return vec![
                    event("discard_recommendation", json!([{"yaku":"souzu_switch","data":{
                        "status":"impossible","reason":"invalid-search-preferences",
                        "search_algorithm":data.pointer("/options/search_algorithm"),
                        "request_source":if action=="start_debug" {"debug"}else{"live"}
                    }}])),
                    event("souzu_switch_control_result", json!({"action":action,"ok":false,"reason":"invalid-search-preferences"})),
                ],
            };
            let debug = action == "start_debug";
            let algorithm = data
                .get("options")
                .and_then(|v| v.get("search_algorithm"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let searching = json!({
                "status": "searching",
                "request_source": if debug { "debug" } else { "live" },
                "search_algorithm": algorithm,
            });
            let generation = if debug {
                let generation = state.debug_generation.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = state.events.send(event(
                    "discard_recommendation",
                    json!([{"yaku":"souzu_switch","data":searching}]),
                ));
                generation
            } else {
                let mut current = state.switch_plan.write().await;
                state
                    .switch_generation
                    .send_modify(|generation| *generation += 1);
                *current = Some(searching.clone());
                let _ = state.events.send(event(
                    "discard_recommendation",
                    json!([{"yaku":"souzu_switch","data":searching}]),
                ));
                *state.switch_generation.borrow()
            };
            let snapshot = if action == "start_debug" {
                data.get("snapshot")
                    .cloned()
                    .unwrap_or_else(|| state.services.game_state())
            } else {
                state.services.game_state()
            };
            let skip_signatures = data
                .get("options")
                .and_then(|value| value.get("skip_signatures"))
                .and_then(Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let session = snapshot.get("session_id").cloned().unwrap_or(Value::Null);
            let revision = snapshot.get("revision").cloned().unwrap_or(Value::Null);
            let result_algorithm = algorithm.clone();
            let target_groups = data.pointer("/options/target_groups").cloned().unwrap_or(Value::Null);
            let worker_state = state.clone();
            let mut plan = tokio::task::spawn_blocking(move || {
                let stopped = || if debug {
                    worker_state.debug_generation.load(Ordering::SeqCst) != generation
                } else {
                    *worker_state.switch_generation.borrow() != generation
                };
                let mut progress = |progress| {
                        if stopped() { return; }
                        let _ = worker_state.events.send(event("discard_recommendation", json!([{
                            "yaku":"souzu_switch", "data": {
                                "status":"searching", "search_progress":progress,
                                "search_algorithm":algorithm,
                                "request_source":if debug {"debug"} else {"live"},
                            }
                        }])));
                    };
                if algorithm.as_deref() == Some("custom_target") {
                    recommendations::custom_switch_plan(&snapshot, &target_groups, wall_limit, &mut progress, &stopped)
                } else {
                    recommendations::switch_plan_with_preferences(&snapshot, wall_limit, &skip_signatures,
                        algorithm.as_deref(), &preferences, &mut progress, &stopped)
                }
            })
            .await
            .unwrap_or_else(|error| {
                json!({"status":"impossible","reason":format!("search-task-failed: {error}")})
            });
            let mut current = state.switch_plan.write().await;
            let current_generation = if debug {
                state.debug_generation.load(Ordering::SeqCst)
            } else {
                *state.switch_generation.borrow()
            };
            if current_generation != generation
                || (!debug && state.services.game_state()["session_id"] != session)
            {
                return vec![];
            }
            if let Some(object) = plan.as_object_mut() {
                object.insert(
                    "request_source".into(),
                    Value::String(
                        if action == "start_debug" {
                            "debug"
                        } else {
                            "live"
                        }
                        .into(),
                    ),
                );
            }
            plan["plan_id"] = json!(format!(
                "{}-{generation}",
                if debug { "debug" } else { "live" }
            ));
            if let Some(algorithm) = result_algorithm {
                plan["search_algorithm"] = json!(algorithm);
            }
            plan["generation"] = json!(generation);
            plan["session_id"] = session;
            plan["state_revision"] = revision;
            if !debug {
                *current = Some(plan.clone());
            }
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":plan}]),
            )]
        }
        "list_quads" => {
            let catalog = recommendations::quad_catalog(&state.services.game_state(), wall_limit);
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":{"status":"catalog","quad_catalog":catalog}}]),
            )]
        }
        "validate_manual_debug" => {
            state.debug_generation.fetch_add(1, Ordering::SeqCst);
            let snapshot = data
                .get("snapshot")
                .cloned()
                .unwrap_or_else(|| state.services.game_state());
            let mut plan = recommendations::validate_manual_plan(
                &snapshot,
                wall_limit,
                data.get("quad_groups").unwrap_or(&Value::Null),
                data.get("structure_groups").unwrap_or(&Value::Null),
            );
            if let Some(object) = plan.as_object_mut() {
                object.insert("request_source".into(), Value::String("debug".into()));
            }
            vec![event(
                "discard_recommendation",
                json!([{"yaku":"souzu_switch","data":plan}]),
            )]
        }
        "execute_plan" | "execute_full_plan" => {
            let reject = |reason: &str| {
                vec![event(
                    "souzu_switch_control_result",
                    json!({"action":action,"ok":false,"reason":reason}),
                )]
            };
            let Ok(_execution) = state.switch_execution.try_lock() else {
                return reject("execution-already-running");
            };
            let plan = state.switch_plan.read().await.clone();
            let Some(plan) = plan else {
                return vec![event(
                    "souzu_switch_control_result",
                    json!({"action":action,"ok":false,"reason":"no-plan"}),
                )];
            };
            let mut cancellation = state.switch_generation.subscribe();
            let requested_id = data
                .get("options")
                .and_then(|v| v.get("plan_id"))
                .and_then(Value::as_str);
            let game = state.services.game_state();
            if requested_id.is_none()
                || requested_id != plan.get("plan_id").and_then(Value::as_str)
                || plan.get("request_source").and_then(Value::as_str) != Some("live")
                || plan.get("state_revision") != game.get("revision")
                || !plan_is_current(state, &plan).await
            {
                return reject("stale-plan");
            }
            let execute = async {
                if action == "execute_full_plan" {
                    execute_full_switch_plan(state, &plan).await
                } else {
                    execute_switch_batches(state, &plan, false).await
                }
            };
            let result = tokio::select! {
                biased;
                _ = cancellation.changed() => Err("stopped-by-user".into()),
                result = execute => result,
            };
            let (ok, reason) = match result {
                Ok(()) => (true, String::new()),
                Err(reason) => (false, reason),
            };
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":if ok{"completed"}else{"failed"},"phase":"done","reason":reason}),
            ));
            vec![event(
                "souzu_switch_control_result",
                json!({"action":action,"ok":ok,"reason":reason}),
            )]
        }
        "stop" => {
            let mut current = state.switch_plan.write().await;
            state
                .switch_generation
                .send_modify(|generation| *generation += 1);
            state.debug_generation.fetch_add(1, Ordering::SeqCst);
            let mut stopped = current
                .take()
                .unwrap_or_else(|| json!({"request_source":"live"}));
            stopped["status"] = json!("impossible");
            stopped["reason"] = json!("stopped-by-user");
            vec![event(
                "discard_recommendation",
                json!([
                    {"yaku":"souzu_switch","data":stopped},
                    {"yaku":"souzu_switch","data":{"status":"impossible","reason":"stopped-by-user","request_source":"debug"}},
                ]),
            )]
        }
        _ => vec![event(
            "souzu_switch_control_result",
            json!({"action":action,"ok":false,"reason":"unknown-action"}),
        )],
    }
}

fn value_ids(value: Option<&Value>) -> Vec<u64> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default()
}

fn operation_available(game: &Value, operation_type: u64) -> bool {
    game.get("next_operation")
        .and_then(Value::as_array)
        .is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation.get("type").and_then(Value::as_u64) == Some(operation_type)
            })
        })
}

fn deck_face(game: &Value, id: u64) -> Option<&str> {
    game.get("deck_map")?.get(id.to_string())?.as_str()
}

pub(super) async fn plan_is_current(state: &State, plan: &Value) -> bool {
    let id = plan.get("plan_id").and_then(Value::as_str);
    id.is_some()
        && plan.get("generation").and_then(Value::as_u64) == Some(*state.switch_generation.borrow())
        && plan.get("session_id") == state.services.game_state().get("session_id")
        && state
            .switch_plan
            .read()
            .await
            .as_ref()
            .is_some_and(|current| current.get("plan_id").and_then(Value::as_str) == id)
}

async fn wait_for_game_change(
    state: &State,
    plan: &Value,
    old_hand: &[u64],
    old_change_count: u64,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        let change_count = game
            .get("change_tile_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if hand != old_hand || change_count != old_change_count {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("game-state-update-timeout".into())
}

async fn send_game_operation(
    state: &State,
    operation_type: u64,
    tile_list: &[u64],
) -> Result<(), String> {
    let game = state.services.game_state();
    if game["source"] == "qyzz" {
        return state.qyzz.operate(&game, operation_type, tile_list).await;
    }
    match state
        .proxy
        .request_with_retry(
            ".lq.Lobby.amuletActivityGameOperate",
            &json!({"activityId":260511,"type":operation_type,"tileList":tile_list}),
            Duration::from_secs(12),
        )
        .await
    {
        Ok((_, response)) if response.get("error").is_none() => Ok(()),
        Ok((_, response)) => Err(format!(
            "protocol-error: {}",
            response.get("error").unwrap_or(&Value::Null)
        )),
        Err(error) => Err(error),
    }
}

async fn execute_switch_batches(
    state: &State,
    plan: &Value,
    finish_switch: bool,
) -> Result<(), String> {
    if plan.get("status").and_then(Value::as_str) != Some("plan") {
        return Err("plan-not-executable".into());
    }
    let raw = plan
        .get("switch_discards")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let batches = if raw.first().is_some_and(Value::is_number) {
        vec![Value::Array(raw)]
    } else {
        raw
    };
    for (index, batch) in batches.iter().enumerate() {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let discard_ids = value_ids(Some(batch));
        // The v2.4.1 simulator retains rounds in which no tiles need replacing.
        if discard_ids.is_empty() {
            continue;
        }
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        if discard_ids.is_empty() || discard_ids.iter().any(|id| !hand.contains(id)) {
            return Err("switch-plan-no-longer-matches-hand".into());
        }
        let keep = hand
            .iter()
            .copied()
            .filter(|id| !discard_ids.contains(id))
            .collect::<Vec<_>>();
        let old_change_count = game
            .get("change_tile_count")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let _ = state.services.events.send(event(
            "souzu_switch_execution",
            json!({"status":"running","phase":"switch","batch_index":index+1,"batch_total":batches.len(),"batch_count":batches.len()}),
        ));
        send_game_operation(state, 101, &keep).await?;
        wait_for_game_change(
            state,
            plan,
            &hand,
            old_change_count,
            Duration::from_secs(12),
        )
        .await?;
    }
    if !finish_switch {
        return Ok(());
    }

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        // The last exchange can enter play directly, without offering a finish operation.
        if game["stage"] == 5
            || [1, 4, 8].iter().any(|kind| operation_available(&game, *kind))
        {
            return Ok(());
        }
        if operation_available(&game, 100) {
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"finish-switch"}),
            ));
            if game["source"] == "qyzz" {
                return state.qyzz.operate(&game, 100, &[]).await;
            }
            match state
                .proxy
                .request_with_retry(
                    ".lq.Lobby.amuletActivityOperate",
                    &json!({"activityId":260511,"type":3,"args":[]}),
                    Duration::from_secs(12),
                )
                .await
            {
                Ok((_, response)) if response.get("error").is_none() => return Ok(()),
                Ok((_, response)) => {
                    return Err(format!(
                        "protocol-error: {}",
                        response.get("error").unwrap_or(&Value::Null)
                    ))
                }
                Err(error) => return Err(error),
            }
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("finish-switch-operation-timeout".into())
}

fn choose_discard(game: &Value, plan: &Value, pending_quads: &[String]) -> Option<u64> {
    let hand = value_ids(game.get("hand_tiles"));
    if let Some(planned) = plan.get("post_draw_discards").and_then(Value::as_array) {
        if let Some(id) = planned
            .iter()
            .filter_map(Value::as_u64)
            .find(|id| hand.contains(id))
        {
            return Some(id);
        }
    }
    let mut wanted = HashMap::<String, usize>::new();
    for face in plan
        .get("target13")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        *wanted.entry(face.to_owned()).or_default() += 1;
    }
    for face in pending_quads {
        *wanted.entry(face.clone()).or_default() += 4;
    }
    let mut kept = HashMap::<String, usize>::new();
    for id in hand.iter().copied() {
        let face = deck_face(game, id).unwrap_or("");
        let count = kept.entry(face.to_owned()).or_default();
        if *count >= wanted.get(face).copied().unwrap_or(0) {
            return Some(id);
        }
        *count += 1;
    }
    None
}

async fn execute_full_switch_plan(state: &State, plan: &Value) -> Result<(), String> {
    if plan
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.starts_with("wanxiang"))
    {
        return Err("wanxiang-plan-has-no-full-kan-execution".into());
    }
    execute_switch_batches(state, plan, true).await?;
    let mut pending_quads = plan
        .get("quad_faces")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !(2..=3).contains(&pending_quads.len()) {
        return Err("full-plan-requires-two-or-three-quads".into());
    }
    let draws_needed = plan
        .get("draws_needed")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut post_kan_discards = 0usize;
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        let hand = value_ids(game.get("hand_tiles"));
        if let Some((quad_index, quad_ids)) =
            pending_quads.iter().enumerate().find_map(|(index, face)| {
                let ids = hand
                    .iter()
                    .copied()
                    .filter(|id| deck_face(&game, *id) == Some(face.as_str()))
                    .take(4)
                    .collect::<Vec<_>>();
                (ids.len() == 4).then_some((index, ids))
            })
        {
            if operation_available(&game, 4) {
                let _ = state.services.events.send(event(
                    "souzu_switch_execution",
                    json!({"status":"running","phase":"kan","quad_face":pending_quads[quad_index]}),
                ));
                send_game_operation(state, 4, &quad_ids).await?;
                let change_count = game
                    .get("change_tile_count")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                wait_for_game_change(state, plan, &hand, change_count, Duration::from_secs(12))
                    .await?;
                pending_quads.remove(quad_index);
                continue;
            }
        }
        if pending_quads.is_empty()
            && (operation_available(&game, 8)
                || game
                    .get("ting_list")
                    .and_then(Value::as_array)
                    .is_some_and(|values| !values.is_empty())
                || post_kan_discards >= draws_needed)
        {
            return Ok(());
        }
        if operation_available(&game, 1) {
            let discard = choose_discard(&game, plan, &pending_quads).ok_or("no-safe-discard")?;
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"discard","tile_id":discard}),
            ));
            send_game_operation(state, 1, &[discard]).await?;
            let change_count = game
                .get("change_tile_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            wait_for_game_change(state, plan, &hand, change_count, Duration::from_secs(12)).await?;
            if pending_quads.is_empty() {
                post_kan_discards += 1;
            }
            continue;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("full-plan-execution-timeout".into())
}

#[cfg(test)]
mod three_quad_execution_tests {
    use super::*;
    #[test]
    fn discard_preserves_pending_quads_and_the_final_wait() {
        let game = json!({"hand_tiles":[1,2,3,4,5],"deck_map":{"1":"1m","2":"1m","3":"4p","4":"7z","5":"9s"}});
        let plan = json!({"target13":["4p","7z"]});
        assert_eq!(choose_discard(&game, &plan, &["1m".into()]), Some(5));
        assert_eq!(choose_discard(&game, &plan, &[]), Some(1));
        let full = json!({"target13":["1m","1m","4p","7z","9s"]});
        assert_eq!(choose_discard(&game, &full, &[]), None);
    }
}
