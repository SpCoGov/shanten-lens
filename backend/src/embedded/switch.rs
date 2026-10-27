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
            let exclude_invalid_shapes = data.pointer("/options/exclude_invalid_shapes").cloned().unwrap_or(json!(false));
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
                    match serde_json::from_value::<bool>(exclude_invalid_shapes) {
                        Ok(exclude) => recommendations::custom_switch_plan(&snapshot, &target_groups, wall_limit, exclude, &mut progress, &stopped),
                        Err(_) => json!({"status":"impossible","reason":"custom-invalid-options"}),
                    }
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
            let reason_key = match reason.as_str() {
                "execution-state-mismatch" => "blackhole.execute_reason_state_mismatch",
                "execution-game-ended" => "blackhole.execute_reason_game_ended",
                "blackhole.execute_reason_exchange_limit" => "blackhole.execute_reason_exchange_limit",
                "blackhole.execute_reason_hand_modified" => "blackhole.execute_reason_hand_modified",
                _ => "",
            };
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":if ok{"completed"}else{"failed"},"phase":"done","reason":reason,"reason_key":reason_key,
                    "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                    "execution_kind":if action=="execute_full_plan" {"full_plan"}else{"switch"}}),
            ));
            vec![event(
                "souzu_switch_control_result",
                json!({"action":action,"ok":ok,"reason":reason,"reason_key":reason_key}),
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

pub(super) fn operation_available(game: &Value, operation_type: u64) -> bool {
    // Packet stage 5 advertises only exchange (101); finishing uses Lobby.amuletActivityOperate.
    if operation_type == 100 && game["source"] == "packet" && game["stage"] == 5 && super::round::in_round(game) {
        return true;
    }
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
    let game = state.services.game_state();
    id.is_some()
        && plan.get("generation").and_then(Value::as_u64) == Some(*state.switch_generation.borrow())
        && plan.get("session_id") == game.get("session_id")
        && plan.get("round_level").is_none_or(|level| *level == game["level"])
        && plan.get("round_node").is_none_or(|node| *node == game["node"])
        && plan.get("round_generation").is_none_or(|generation| generation.as_u64()
            .is_some_and(|generation| state.automation.round_is_current(generation) && super::round::in_round(&game)))
        && state
            .switch_plan
            .read()
            .await
            .as_ref()
            .is_some_and(|current| current.get("plan_id").and_then(Value::as_str) == id)
}

pub(super) async fn send_game_operation(
    state: &State,
    plan: &Value,
    game: &Value,
    operation_type: u64,
    tile_list: &[u64],
) -> Result<(), String> {
    if !plan_is_current(state, plan).await {
        return Err("stopped-by-user".into());
    }
    let current = state.services.game_state();
    if (current["revision"] != game["revision"] && !super::qyzz::same_operation_state(game, &current))
        || current["session_id"] != game["session_id"]
        || game["ended"] == true || !operation_available(game, operation_type) {
        return Err("execution-state-mismatch".into());
    }
    if operation_type == 101 {
        let hand = value_ids(game.get("hand_tiles"));
        let keep = tile_list.iter().copied().collect::<std::collections::HashSet<_>>();
        if keep.len() != tile_list.len() || keep.iter().any(|id| !hand.contains(id)) {
            return Err("switch-plan-no-longer-matches-hand".into());
        }
        if hand.len().saturating_sub(keep.len()) > recommendations::search_limits(game).1 {
            return Err("blackhole.execute_reason_exchange_limit".into());
        }
    }
    if game["source"] == "qyzz" {
        return state.qyzz.operate(&state.services, game, operation_type, tile_list).await;
    }
    // A resync may change the hand; never replay a mutating request against the new state.
    // Packet transport ends exchanges through Lobby Operate; Qyzz uses operation 100 above.
    let (method, data) = if operation_type == 100 {
        (".lq.Lobby.amuletActivityOperate", json!({"activityId":261081,"type":3,"args":[]}))
    } else {
        (".lq.Lobby.amuletActivityGameOperate", json!({"activityId":261081,"type":operation_type,"tileList":tile_list}))
    };
    match state
        .proxy
        .request(method, &data, Duration::from_secs(12))
        .await
    {
        Ok((_, response)) if response.get("error").is_none() => {
            let changed_by_effect = operation_type == 101 && response["events"].as_array().is_some_and(|events|
                events.iter().flat_map(|event| event["effectedHooks"].as_array().into_iter().flatten())
                    .any(|hook| hook["result"]["modifyChangeHandsList"].as_array().is_some_and(|hand| !hand.is_empty())));
            if changed_by_effect {
                let current = state.services.game_state();
                let mut expected = tile_list.to_vec();
                expected.extend(value_ids(game.get("replacement_tiles")).into_iter()
                    .skip(value_ids(game.get("switch_used_tiles")).len())
                    .take(value_ids(game.get("hand_tiles")).len().saturating_sub(tile_list.len())));
                let mut actual = value_ids(current.get("hand_tiles"));
                expected.sort_unstable();
                actual.sort_unstable();
                if expected != actual && current["change_tile_count"].as_u64() > game["change_tile_count"].as_u64() {
                    return Err("blackhole.execute_reason_hand_modified".into());
                }
            }
            Ok(())
        },
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
        let incoming_batches = plan.get("switch_in");
        let incoming = value_ids(if incoming_batches.and_then(|batches| batches.get(0)).is_some_and(Value::is_number) {
            incoming_batches
        } else { incoming_batches.and_then(|batches| batches.get(index)) });
        if incoming.len() != discard_ids.len() {
            return Err("switch-plan-no-longer-matches-hand".into());
        }
        let mut expected = keep.clone();
        expected.extend(incoming);
        let mut expected_hands = vec![expected.clone()];
        if let Some(draw) = value_ids(game.get("wall_tiles")).first() {
            expected.push(*draw);
            expected_hands.push(expected);
        }
        // Automatic plans report progress through autorun_status, without opening the manual dialog.
        if plan["round_generation"].is_null() {
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"switch","phase_key":"blackhole.execute_phase_sending",
                    "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                    "execution_kind":if finish_switch {"full_plan"}else{"switch"},
                    "batch_index":index+1,"batch_total":batches.len(),"batch_count":batches.len()}),
            ));
        }
        send_game_operation(state, plan, &game, 101, &keep).await?;
        wait_for_execution_state(state, plan, &expected_hands, &meld_ids(&game), None).await?;
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
        if [1, 4, 8].iter().any(|kind| operation_available(&game, *kind))
        {
            wait_for_play_turn(state, plan, &game).await?;
            return Ok(());
        }
        if operation_available(&game, 100) {
            if plan["round_generation"].is_null() {
                let _ = state.services.events.send(event(
                    "souzu_switch_execution",
                    json!({"status":"running","phase":"finish-switch","execution_kind":"full_plan",
                        "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                        "phase_key":"blackhole.execute_phase_finish_switch"}),
                ));
            }
            send_game_operation(state, plan, &game, 100, &[]).await?;
            wait_for_play_turn(state, plan, &game).await?;
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("finish-switch-operation-timeout".into())
}

fn choose_discard(game: &Value, plan: &Value, pending_quads: &[String]) -> Option<u64> {
    let hand = value_ids(game.get("hand_tiles"));
    let mut wanted = HashMap::<String, usize>::new();
    for face in plan
        .get("target13")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        *wanted.entry(recommendations::norm(face)).or_default() += 1;
    }
    for face in pending_quads {
        *wanted.entry(recommendations::norm(face)).or_default() += 4;
    }
    let mut kept = HashMap::<String, usize>::new();
    let mut safe = Vec::new();
    for id in hand.iter().copied() {
        let face = recommendations::norm(deck_face(game, id)?);
        if face == "bd" { continue; }
        let count = kept.entry(face.clone()).or_default();
        if *count >= wanted.get(&face).copied().unwrap_or(0) {
            safe.push(id);
        } else {
            *count += 1;
        }
    }
    value_ids(plan.get("post_draw_discards")).into_iter().find(|id| safe.contains(id))
        .or_else(|| safe.first().copied())
}

async fn execute_full_switch_plan(state: &State, plan: &Value) -> Result<(), String> {
    if plan["search_algorithm"] == "custom_target"
        || (plan["search_algorithm"] == "wanxiang_four_meld_switch"
            && plan["custom_steps"].is_array()
            && serde_json::from_value::<recommendations::SearchPreferences>(plan["search_preferences"].clone())
                .is_ok_and(|preferences| preferences.coin_objective.is_some()))
    {
        return execute_custom_switch_plan(state, plan).await;
    }
    if plan
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.starts_with("wanxiang"))
    {
        return Err("wanxiang-plan-has-no-full-kan-execution".into());
    }
    let mut pending_quads = plan
        .get("quad_faces")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(recommendations::norm)
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
    let wall_size = value_ids(state.services.game_state().get("wall_tiles")).len();
    execute_switch_batches(state, plan, true).await?;
    let mut game = state.services.game_state();
    let mut draws = wall_size.saturating_sub(value_ids(game.get("wall_tiles")).len());
    loop {
        game = wait_for_execution_state(state, plan, &[value_ids(game.get("hand_tiles"))], &meld_ids(&game), None).await?;
        let hand = value_ids(game.get("hand_tiles"));
        if let Some((quad_index, quad_ids)) =
            pending_quads.iter().enumerate().find_map(|(index, face)| {
                let ids = hand
                    .iter()
                    .copied()
                    .filter(|id| deck_face(&game, *id).is_some_and(|raw| recommendations::norm(raw) == *face))
                    .take(4)
                    .collect::<Vec<_>>();
                (ids.len() == 4).then_some((index, ids))
            })
        {
            // Keep a complete quad intact while the operation list catches up.
            game = wait_for_execution_state(state, plan, &[hand], &meld_ids(&game), Some(4)).await?;
            let _ = state.services.events.send(event(
                "souzu_switch_execution",
                json!({"status":"running","phase":"kan","execution_kind":"full_plan",
                    "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                    "phase_key":"blackhole.execute_phase_kan","quad_face":pending_quads[quad_index]}),
            ));
            game = execute_play_operation(state, plan, &game, 4, &quad_ids).await?;
            draws += 1;
            pending_quads.remove(quad_index);
            continue;
        }
        if draws >= draws_needed + 2 {
            return Err("execution-state-mismatch".into());
        }
        game = wait_for_execution_state(state, plan, &[hand.clone()], &meld_ids(&game), Some(1)).await?;
        let discard = choose_discard(&game, plan, &pending_quads).ok_or("no-safe-discard")?;
        let mut retained = hand.iter().filter(|&&id| id != discard)
            .map(|&id| deck_face(&game, id).map(recommendations::norm).unwrap_or_default()).collect::<Vec<_>>();
        let mut target = plan["target13"].as_array().into_iter().flatten()
            .filter_map(Value::as_str).map(recommendations::norm).collect::<Vec<_>>();
        retained.sort();
        target.sort();
        let final_discard = pending_quads.is_empty() && retained == target;
        if final_discard && (operation_available(&game, 8)
            || game["ting_list"].as_array().is_some_and(|values| !values.is_empty())) {
            return Ok(());
        }
        let _ = state.services.events.send(event(
            "souzu_switch_execution",
            json!({"status":"running","phase":"discard","execution_kind":"full_plan",
                "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                "phase_key":"blackhole.execute_phase_auto_discard","reason_values":{"tile_id":discard}}),
        ));
        game = execute_play_operation(state, plan, &game, 1, &[discard]).await?;
        draws += 1;
        if final_discard { return Ok(()); }
    }
}

fn meld_ids(game: &Value) -> Vec<Vec<u64>> {
    let mut melds = game["ming"].as_array().into_iter().flatten().map(|meld| {
        let mut tiles = value_ids(meld.get("tileList"));
        tiles.sort_unstable();
        tiles
    }).collect::<Vec<_>>();
    melds.sort_unstable();
    melds
}

async fn wait_for_execution_state(
    state: &State,
    plan: &Value,
    expected_hands: &[Vec<u64>],
    expected_melds: &[Vec<u64>],
    operation: Option<u64>,
) -> Result<Value, String> {
    let mut expected_hands = expected_hands.to_vec();
    for hand in &mut expected_hands { hand.sort_unstable(); }
    let mut expected_melds = expected_melds.to_vec();
    for meld in &mut expected_melds { meld.sort_unstable(); }
    expected_melds.sort_unstable();
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        if !plan_is_current(state, plan).await {
            return Err("stopped-by-user".into());
        }
        let game = state.services.game_state();
        if game["ended"] == true {
            return Err("execution-game-ended".into());
        }
        let mut hand = value_ids(game.get("hand_tiles"));
        hand.sort_unstable();
        if expected_hands.contains(&hand) && meld_ids(&game) == expected_melds
            && operation.is_none_or(|kind| operation_available(&game, kind)) {
            return Ok(game);
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    Err("execution-state-mismatch".into())
}

async fn wait_for_play_turn(state: &State, plan: &Value, game: &Value) -> Result<Value, String> {
    let mut hand = value_ids(game.get("hand_tiles"));
    let melds = meld_ids(game);
    if hand.len() + melds.len() * 3 == 13 {
        hand.push(*value_ids(game.get("wall_tiles")).first().ok_or("execution-state-mismatch")?);
    }
    if hand.len() + melds.len() * 3 != 14 { return Err("execution-state-mismatch".into()); }
    wait_for_execution_state(state, plan, &[hand], &melds, Some(1)).await
}

async fn execute_play_operation(state: &State, plan: &Value, game: &Value, kind: u64, tiles: &[u64]) -> Result<Value, String> {
    let mut hand = value_ids(game.get("hand_tiles"));
    let mut melds = meld_ids(game);
    if !operation_available(game, kind) || hand.len() + melds.len() * 3 != 14
        || !matches!((kind, tiles.len()), (1, 1) | (4, 4))
        || tiles.iter().enumerate().any(|(i, id)| tiles[..i].contains(id)
            || !hand.contains(id) || deck_face(game, *id).is_none_or(|face| face == "bd"))
        || (kind == 4 && tiles.iter().any(|&id| recommendations::norm(deck_face(game, id).unwrap_or(""))
            != recommendations::norm(deck_face(game, tiles[0]).unwrap_or("")))) {
        return Err("execution-state-mismatch".into());
    }
    hand.retain(|id| !tiles.contains(id));
    if kind == 4 { melds.push(tiles.to_vec()); }
    if let Some(draw) = value_ids(game.get("wall_tiles")).first() { hand.push(*draw); }
    send_game_operation(state, plan, game, kind, tiles).await?;
    // A reply or a changed hand is not proof that the requested kan/discard was applied.
    wait_for_execution_state(state, plan, &[hand], &melds, None).await
}

pub(super) async fn execute_custom_switch_plan(state: &State, plan: &Value) -> Result<(), String> {
    if plan.get("state_revision") != state.services.game_state().get("revision") {
        return Err("stale-plan".into());
    }
    let steps = plan["custom_steps"].as_array().ok_or("custom-missing-steps")?;
    let mut hand = value_ids(plan.get("initial_hand"));
    let mut melds = meld_ids(&state.services.game_state());
    if hand.is_empty() {
        return Err("custom-missing-hand".into());
    }
    if !steps.is_empty() || operation_available(&state.services.game_state(), 100) {
        execute_switch_batches(state, plan, true).await?;
    }
    for step in steps {
        let tiles = value_ids(step.get("tiles"));
        let kind = step["kind"].as_str().ok_or("custom-invalid-step")?;
        if kind == "exchange" {
            let out = value_ids(step.get("out"));
            hand.retain(|id| !out.contains(id));
            hand.extend(tiles);
            continue;
        }
        let operation = match kind {
            "draw" => None,
            "discard" if tiles.len() == 1 => Some(1),
            "kan" if tiles.len() == 4 => Some(4),
            _ => return Err("custom-invalid-step".into()),
        };
        if plan["round_generation"].is_null() {
            let _ = state.services.events.send(event("souzu_switch_execution", json!({
                "status":"running", "phase":kind, "execution_kind":"full_plan",
                "search_algorithm":plan["search_algorithm"],"plan_id":plan["plan_id"],
                "phase_key":format!("blackhole.execute_phase_{kind}"),
                "reason_values":{"tile_id":tiles.first()},
            })));
        }
        if kind == "draw" {
            // Finishing exchanges, discarding and declaring kan draw automatically.
            hand.extend(tiles);
            wait_for_execution_state(state, plan, &[hand.clone()], &melds, None).await?;
        } else {
            let game = wait_for_execution_state(state, plan, &[hand.clone()], &melds, operation).await?;
            if tiles.iter().any(|id| !hand.contains(id) || deck_face(&game, *id) == Some("bd")) {
                return Err("custom-step-no-longer-matches-hand".into());
            }
            execute_play_operation(state, plan, &game, operation.unwrap(), &tiles).await?;
            if kind == "kan" { melds.push(tiles.clone()); }
            hand.retain(|id| !tiles.contains(id));
        }
    }
    let game = wait_for_execution_state(state, plan, &[hand.clone()], &melds, None).await?;
    let mut actual = hand;
    for meld in game["ming"].as_array().into_iter().flatten() {
        actual.extend(value_ids(meld.get("tileList")));
    }
    let mut target = value_ids(plan.get("target_physical_ids"));
    actual.sort_unstable();
    target.sort_unstable();
    if actual != target {
        return Err("custom-target-mismatch".into());
    }
    Ok(())
}

#[cfg(test)]
mod three_quad_execution_tests {
    use super::*;
    #[test]
    fn packet_exchange_stage_can_finish_without_synthetic_operation() {
        let mut game = json!({"source":"packet","stage":5,"action_report":{"interrupt":0},"next_operation":[{"type":101}]});
        assert!(operation_available(&game, 100));
        game["action_report"]["interrupt"] = json!(1000);
        assert!(!operation_available(&game, 100));
        game["stage"] = json!(6);
        assert!(!operation_available(&game, 100));
    }
    #[test]
    fn discard_preserves_pending_quads_and_the_final_wait() {
        let game = json!({"hand_tiles":[1,2,3,4,5],"deck_map":{"1":"1m","2":"1m","3":"4p","4":"7z","5":"9s"}});
        let plan = json!({"target13":["4p","7z"]});
        assert_eq!(choose_discard(&game, &plan, &["1m".into()]), Some(5));
        assert_eq!(choose_discard(&game, &plan, &[]), Some(1));
        let full = json!({"target13":["1m","1m","4p","7z","9s"]});
        assert_eq!(choose_discard(&game, &full, &[]), None);
        let red_quad = json!({"hand_tiles":[1,2,3,4,5,1000],"deck_map":{"1":"5p","2":"0p","3":"5p","4":"5p","5":"9s","1000":"bd"}});
        let planned = json!({"target13":[],"post_draw_discards":[2,1000]});
        assert_eq!(choose_discard(&red_quad, &planned, &["5p".into()]), Some(5));
    }
}
