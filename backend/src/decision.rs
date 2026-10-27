//! Read-only rule contributions for legal actions; execution remains explicit.
use crate::action_provider::{ActionCandidate, ActionReason, ActionReport, Availability, LEVELS, RULES as ACTION_RULES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use specta::Type;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct RuleScore {
    pub rule_id: String,
    pub score: Option<f64>,
    pub reason: ActionReason,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ActionScore {
    pub action_id: String,
    pub total: Option<f64>,
    pub rules: Vec<RuleScore>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct DecisionReport {
    pub actions: Vec<ActionScore>,
}

pub(crate) fn first_action(state: &Value) -> Option<ActionCandidate> {
    let scores = state["decision_report"]["actions"].as_array();
    let mut best: Option<(&Value, f64)> = None;
    for action in ["actions", "global_actions"].iter()
        .flat_map(|key| state["action_report"][key].as_array().into_iter().flatten()) {
        if action["availability"] != "available" { continue; }
        let score = scores.and_then(|scores| scores.iter().find(|score| score["action_id"] == action["id"]))
            .and_then(|score| score["total"].as_f64()).unwrap_or(0.0);
        // Match the UI's stable descending sort, including zero for absent scores.
        if best.is_none_or(|(_, current)| score > current) { best = Some((action, score)); }
    }
    best.and_then(|(action, _)| serde_json::from_value(action.clone()).ok())
}

#[cfg(test)]
#[test]
fn first_action_uses_stable_scores_and_skips_illegal_actions() {
    let action = |id: &str, availability: &str| json!({"id":id,"kind":id,"params":{},
        "cost":null,"cost_resource":"coin","availability":availability,"reasons":[]});
    let mut state = json!({"action_report":{
        "actions":[action("first", "available"),action("second", "available"),action("illegal", "unavailable")],
        "global_actions":[action("remake", "available")]},"decision_report":{"actions":[
            {"action_id":"illegal","total":1000},{"action_id":"first","total":null},
            {"action_id":"second","total":0},{"action_id":"remake","total":-1}]}});
    assert_eq!(first_action(&state).unwrap().id, "first");
    state["decision_report"]["actions"][2]["total"] = json!(30);
    assert_eq!(first_action(&state).unwrap().id, "second");
    state["decision_report"]["actions"][3]["total"] = json!(50);
    assert_eq!(first_action(&state).unwrap().id, "remake");
    state["action_report"]["actions"] = json!([]);
    state["action_report"]["global_actions"][0]["availability"] = json!("unknown");
    assert!(first_action(&state).is_none());
}

struct Rule {
    id: &'static str,
    action_kind: Option<&'static str>,
    evaluate: fn(&Value, &ActionCandidate, &Value, &Value) -> Option<(Option<f64>, &'static str)>,
}

const RULES: &[Rule] = &[Rule {
    id: "ryan_without_plus",
    action_kind: None,
    evaluate: ryan_without_plus,
}, Rule {
    id: "coin_cost",
    action_kind: None,
    evaluate: coin_cost,
}, Rule {
    id: "hp_loss",
    action_kind: None,
    evaluate: hp_loss,
}, Rule {
    id: "guidance",
    action_kind: None,
    evaluate: guidance,
}, Rule {
    id: "happiness",
    action_kind: None,
    evaluate: happiness,
}, Rule {
    id: "gamble_outcomes",
    action_kind: Some("gamble"),
    evaluate: gamble_outcomes,
}, Rule {
    id: "amulet_price",
    action_kind: Some("select_candidate"),
    evaluate: amulet_price,
}, Rule {
    id: "pack_value",
    action_kind: Some("buy_pack"),
    evaluate: pack_value,
}, Rule {
    id: "gold_pack_bonus",
    action_kind: Some("buy_pack"),
    evaluate: gold_pack_bonus,
}, Rule {
    id: "configured_target",
    action_kind: None,
    evaluate: configured_target,
}, Rule {
    id: "remake_at_cutoff",
    action_kind: Some("remake"),
    evaluate: remake_at_cutoff,
}, Rule {
    id: "remake_penalty",
    action_kind: Some("remake"),
    evaluate: remake_penalty,
}, Rule {
    id: "upgrade_penalty",
    action_kind: None,
    evaluate: upgrade_penalty,
}, Rule {
    id: "sell_bonus",
    action_kind: Some("sell_effect"),
    evaluate: sell_bonus,
}, Rule {
    id: "sell_amulet_price",
    action_kind: Some("sell_effect"),
    evaluate: amulet_price,
}, Rule {
    id: "retain_amulet",
    action_kind: Some("sell_effect"),
    evaluate: retain_amulet,
}, Rule {
    id: "bonfire",
    action_kind: Some("bonfire"),
    evaluate: bonfire,
}, Rule {
    id: "imp_gain",
    action_kind: None,
    evaluate: imp_gain,
}];

// shortcut: larger scores stay unknown; use decimal-string scores if costs exceed JS exact integers.
const MAX_EXACT_SCORE: i64 = 9_007_199_254_740_991;

pub fn evaluate(state: &Value, actions: &ActionReport, registry: &Value, playbook: &Value) -> DecisionReport {
    let selecting = actions.actions.iter().chain(&actions.global_actions).any(|action| action.kind == "select_candidate");
    let packs: Vec<_> = actions.actions.iter().chain(&actions.global_actions)
        .filter(|action| action.kind == "buy_pack"
            && matches!(action.params["goods_id"].as_u64(), Some(101..=103 | 111..=113))).collect();
    let affordable_pack = if packs.iter().any(|action| action.availability == Availability::Available) {
        Some(true)
    } else if state["provider_state"]["shop"]["goods"].as_array().is_none()
        || packs.iter().any(|action| action.availability == Availability::Unknown) { None }
    else { Some(false) };
    let actions: Vec<_> = actions.actions.iter().chain(&actions.global_actions)
        .filter(|action| action.availability == Availability::Available).collect();
    let refresh_budget = actions.iter().find(|action| state["stage"] == 9 && action.kind == "refresh_shop")
        .map(|refresh| {
            let cost = match refresh.cost_resource.as_str() {
                "coin" => refresh.cost.as_deref()?.parse::<u128>().ok()?,
                "hp" => 0,
                _ => return None,
            };
            crate::action_provider::compare_amount(&state["provider_state"]["game"]["coin"], cost.checked_add(5)?)
                .map(|order| order.is_gt())
        });
    let mut scores: Vec<_> = actions.iter()
        .map(|action| {
            let mut rules: Vec<_> = RULES.iter()
                .filter(|rule| rule.action_kind.map_or(true, |kind| kind == action.kind))
                .filter_map(|rule| {
                    let (score, code) = (rule.evaluate)(state, action, registry, playbook)?;
                    Some(RuleScore {
                        rule_id: rule.id.into(), score,
                        reason: ActionReason { code: code.into(), values: json!({}) },
                    })
                }).collect();
            if action.kind == "select_candidate" {
                rules.extend(replacement_losses(state, action, registry, playbook));
            }
            let refresh_score = match (action.kind.as_str(), refresh_budget, affordable_pack) {
                ("next_node", _, Some(true)) if state["stage"] == 9 => Some((Some(-5.0), "leave_shop_affordable_pack")),
                ("next_node", Some(Some(true)), _) => Some((Some(-20.0), "leave_shop_refresh")),
                ("refresh_shop", Some(Some(true)), Some(false)) => {
                    let bonus = if action.cost_resource == "coin" {
                        action.cost.as_deref().and_then(|cost| cost.parse::<u64>().ok())
                            .and_then(|cost| cost.checked_add(20)).filter(|bonus| *bonus <= MAX_EXACT_SCORE as u64)
                            .map(|bonus| bonus as f64)
                    } else { Some(20.0) };
                    Some((bonus, if bonus.is_some() { "shop_refresh" } else { "invalid_shop_refresh_score" }))
                },
                ("refresh_shop", Some(Some(true)), None) => Some((None, "missing_shop_packs")),
                ("next_node" | "refresh_shop", Some(None), _) => Some((None, "missing_shop_refresh_balance")),
                _ => None,
            };
            if let Some((score, code)) = refresh_score {
                rules.push(RuleScore { rule_id: "shop_refresh".into(), score,
                    reason: ActionReason { code: code.into(), values: json!({}) } });
            }
            if action.kind == "sell_effect" {
                let score = if selecting { sale_followup(state, action, registry, playbook) } else { Some(0.0) };
                if selecting && score != Some(0.0) {
                    rules.push(RuleScore { rule_id: "sale_followup".into(), score,
                        reason: ActionReason { code: if score.is_some() { "sale_followup" } else { "missing_sale_followup" }.into(),
                            values: json!({}) } });
                }
                if rules.iter().any(|rule| rule.rule_id == "retain_amulet") && score.is_some_and(|score| score <= 0.0) {
                    rules.push(RuleScore { rule_id: "retain_amulet_spare_capacity".into(), score: Some(-10.0),
                        reason: ActionReason { code: "retain_amulet_spare_capacity".into(), values: json!({}) } });
                }
            }
            let total: f64 = rules.iter().filter_map(|rule| rule.score).sum();
            let total = (total.is_finite() && total.abs() <= MAX_EXACT_SCORE as f64).then_some(total);
            ActionScore { action_id: action.id.clone(), total, rules }
        }).collect();
    // Skip depends on completed candidate totals, including all configured-target and cost rules.
    let best = actions.iter().zip(&scores).filter(|(action, _)| action.kind == "select_candidate")
        .map(|(_, score)| score.total.unwrap_or(0.0)).reduce(f64::max);
    if let Some(best) = best {
        for (_, score) in actions.iter().zip(&mut scores).filter(|(action, _)| action.kind == "skip") {
            score.rules.push(RuleScore {
                rule_id: "skip_candidates".into(), score: Some(-best),
                reason: ActionReason { code: "skip_candidates".into(), values: json!({}) },
            });
            let total: f64 = score.rules.iter().filter_map(|rule| rule.score).sum();
            score.total = (total.is_finite() && total.abs() <= MAX_EXACT_SCORE as f64).then_some(total);
        }
    }
    if state["stage"] == 9 && !selecting {
        shop_pack_priorities(state, &packs, &actions, &mut scores);
    }
    DecisionReport { actions: scores }
}

fn shop_pack_priorities(state: &Value, packs: &[&ActionCandidate], actions: &[&ActionCandidate], scores: &mut [ActionScore]) {
    let available: Vec<_> = actions.iter().enumerate().filter(|(index, action)| action.kind == "buy_pack"
        && matches!(action.params["goods_id"].as_u64(), Some(101..=103 | 111..=113))
        && scores[*index].total.is_some() && scores[*index].rules.iter().all(|rule| rule.score.is_some()))
        .map(|(index, action)| (index, action.params["goods_id"].as_u64().unwrap() % 10)).collect();
    if available.is_empty() { return; }
    let unsold: Vec<_> = packs.iter().filter(|pack| !pack.reasons.iter().any(|reason| reason.code == "sold")).collect();
    let costs = unsold.iter().try_fold((0u128, 0u128), |(coin, hp), pack| {
        if pack.availability != Availability::Available { return None; }
        let cost = pack.cost.as_deref()?.parse::<u128>().ok()?;
        match pack.cost_resource.as_str() {
            "coin" => Some((coin.checked_add(cost)?, hp)),
            "hp" => Some((coin, hp.checked_add(cost)?)),
            _ => None,
        }
    });
    let context = &state["provider_state"];
    let can_buy_all = context["shop"]["goods"].is_array() && !unsold.is_empty() && costs.is_some_and(|(coin, hp)|
        (coin == 0 || crate::action_provider::compare_amount(&context["game"]["coin"], coin).is_some_and(|order| !order.is_lt()))
            && (hp == 0 || crate::action_provider::compare_amount(&context["character"]["hp"], hp).is_some_and(|order| order.is_gt())));
    let adjust = |score: &mut ActionScore, target: f64, rule: &str| {
        let delta = score.total.map(|total| target - total).filter(|delta| target.is_finite()
            && target.abs() <= MAX_EXACT_SCORE as f64 && delta.is_finite() && delta.abs() <= MAX_EXACT_SCORE as f64);
        score.rules.push(RuleScore { rule_id: rule.into(), score: delta,
            reason: ActionReason { code: if delta.is_some() { rule } else { "invalid_pack_priority" }.into(), values: json!({}) } });
        let total: f64 = score.rules.iter().filter_map(|rule| rule.score).sum();
        score.total = (total.is_finite() && total.abs() <= MAX_EXACT_SCORE as f64).then_some(total);
    };
    if can_buy_all {
        let lowest = unsold.iter().filter_map(|pack| pack.params["goods_id"].as_u64().map(|id| id % 10)).min();
        let chosen = available.iter().filter(|(_, tier)| Some(*tier) == lowest)
            .max_by(|(left, _), (right, _)| scores[*left].total.unwrap().total_cmp(&scores[*right].total.unwrap()));
        if let Some(&(index, _)) = chosen {
            let other_best = scores.iter().enumerate().filter(|(other, _)| *other != index && actions[*other].kind != "remake"
                && !(actions[*other].kind == "sell_effect" && badge_id(&actions[*other].params["effect"]) == Some(600110)))
                .filter_map(|(_, score)| score.total).fold(0.0, f64::max);
            if scores[index].total.unwrap() <= other_best {
                adjust(&mut scores[index], other_best + 1.0, "pack_purchase_order");
            }
        }
    }
    let best_pack = available.iter().filter_map(|(index, _)| scores[*index].total).reduce(f64::max);
    if let Some(best_pack) = best_pack {
        for (action, score) in actions.iter().zip(scores) {
            if action.kind == "sell_effect" && badge_id(&action.params["effect"]) == Some(600070)
                && score.total.is_some_and(|total| total >= best_pack) {
                adjust(score, best_pack - 1.0, "preserve_guidance_for_packs");
            }
        }
    }
}

fn sale_followup(state: &Value, sale: &ActionCandidate, registry: &Value, playbook: &Value) -> Option<f64> {
    let uid = sale.params["uid"].as_u64()?;
    if state.pointer("/provider_state/effect/packCandidates")?.as_array()?.is_empty() { return None; }
    let mut after = state.clone();
    let owned = after.pointer_mut("/provider_state/effect/effectList")?.as_array_mut()?;
    let mut matches = owned.iter().enumerate().filter(|(_, effect)| effect["uid"].as_u64() == Some(uid));
    let index = matches.next()?.0;
    if matches.next().is_some() || owned[index]["id"] != sale.params["effect"]["id"] { return None; }
    owned.remove(index);
    // shortcut: compare one sale and one choice; extend only when modeling longer sale chains.
    let best_choice = |state: &Value| -> Option<f64> {
        let mut choices = crate::action_provider::evaluate(state);
        if choices.status != "ready" { return None; }
        choices.actions.retain(|action| matches!(action.kind.as_str(), "select_candidate" | "skip"));
        choices.global_actions.clear(); // The nested evaluation cannot recurse into another sale.
        if choices.actions.iter().any(|action| action.availability == Availability::Unknown) { return None; }
        let can_skip = choices.actions.iter().any(|action| action.kind == "skip" && action.availability == Availability::Available);
        let scores = evaluate(state, &choices, registry, playbook);
        let mut best = can_skip.then_some(0.0); // The displayed skip penalty is not a reward.
        for action in choices.actions.iter().filter(|action| action.kind == "select_candidate" && action.availability == Availability::Available) {
            let score = scores.actions.iter().find(|score| score.action_id == action.id)?;
            if score.rules.iter().any(|rule| rule.score.is_none()) { return None; }
            let total = score.total?;
            best = Some(best.map_or(total, |best| best.max(total)));
        }
        Some(best.unwrap_or(0.0)) // No legal choice yields nothing until a sale frees capacity.
    };
    let change = best_choice(&after)? - best_choice(state)?;
    (change.is_finite() && change.abs() <= MAX_EXACT_SCORE as f64).then_some(change)
}

fn coin_cost(_state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    if action.cost_resource != "coin" { return None; }
    let cost = action.cost.as_deref()?;
    let score = cost.parse::<i64>().ok()
        .filter(|cost| (0..=MAX_EXACT_SCORE).contains(cost)).map(|cost| -cost);
    if score == Some(0) { return None; }
    Some((score.map(|score| score as f64), if score.is_some() { "coin_cost" } else { "invalid_coin_cost" }))
}

fn sell_bonus(_state: &Value, _action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    Some((Some(5.0), "sell_bonus"))
}

fn remake_penalty(_state: &Value, _action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    Some((Some(-9999.0), "remake_penalty"))
}

fn retain_amulet(_state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    matches!(action.params["effect"]["id"].as_u64(), Some(620 | 621 | 720 | 721 | 1460 | 1461 | 3560 | 3561))
        .then_some((Some(-10.0), "retain_amulet"))
}

fn upgrade_penalty(_state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    matches!(action.kind.as_str(), "shop_upgrade" | "advanced_upgrade")
        .then_some((Some(-10.0), "upgrade_penalty"))
}

fn bonfire(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    let context = &state["provider_state"];
    let choice = action.params["id"].as_u64()?;
    let (score, reason) = match choice {
        1 => {
            let Some(hp) = crate::action_provider::compare_amount(&context["character"]["hp"], 60) else {
                return Some((None, "missing_bonfire_hp"));
            };
            (if hp.is_lt() { 20.0 } else { 0.0 }, "bonfire_heal")
        }
        2 | 4 => {
            let Some(coin) = crate::action_provider::compare_amount(&context["game"]["coin"], 50) else {
                return Some((None, "missing_bonfire_coin"));
            };
            if choice == 2 { (if coin.is_gt() { 25.0 } else { 5.0 }, "bonfire_max_hp") }
            else { (if coin.is_lt() { 20.0 } else { 10.0 }, "bonfire_coin") }
        }
        5..=9 => {
            let used = context["effect"]["effectList"].as_array().and_then(|effects|
                effects.iter().try_fold(0u64, |sum, effect| sum.checked_add(crate::action_provider::volume(effect)?)));
            let maximum = context["effect"].get("maxEffectVolume").unwrap_or(&context["game"]["maxEffectVolume"]).as_u64();
            let (Some(used), Some(maximum)) = (used, maximum) else {
                return Some((None, "missing_bonfire_capacity"));
            };
            (if used < maximum { 10.0 } else { 5.0 }, "bonfire_amulet")
        }
        _ => return None,
    };
    Some((Some(score), reason))
}

fn configured_target(state: &Value, action: &ActionCandidate, _registry: &Value, book: &Value) -> Option<(Option<f64>, &'static str)> {
    if !matches!(action.kind.as_str(), "select_candidate" | "sell_effect") { return None; }
    let profile = book["profiles"].as_array()?.iter().find(|profile| profile["id"] == book["selected_profile_id"])?;
    let effect = &action.params["effect"];
    let raw_id = effect["id"].as_u64().filter(|id| *id > 0)?;
    let upgrading = action.kind == "select_candidate" && state["provider_state"]["effect"]["effectList"].as_array()
        .is_some_and(|owned| !crate::action_provider::completes_effect_group(effect, owned)
            && crate::action_provider::replaced_effect(effect, owned).is_some());
    let raw_id = if upgrading { ACTION_RULES["effects"][raw_id.to_string()]["upgrade"].as_u64().unwrap_or(raw_id) }
        else { raw_id };
    let badge = badge_id(effect);
    let matches_amulet = |target: &Value| target["id"].as_u64() == Some(raw_id / 10)
        && target["plus"].as_bool().unwrap_or(false) == (raw_id % 10 == 1)
        && target["badge"].as_u64().is_none_or(|id| badge == Some(id));
    let value = if profile["strategy"] == "opening" {
        profile["targets"].as_array()?.iter().filter(|target| match target["kind"].as_str() {
            Some("amulet") => matches_amulet(target),
            Some("badge") => badge.is_some_and(|id| id > 0 && target["id"].as_u64() == Some(id)),
            _ => false,
        }).try_fold(0u64, |sum, target| {
            let value = match target.get("value") { None => 1, Some(value) => value.as_u64()? };
            if value == 0 { return None; }
            sum.checked_add(value)
        })
    } else {
        let stage = match tactical_stage(state, profile) {
            Ok(stage) => stage,
            Err(reason) => return Some((None, reason)),
        };
        Some(u64::from(if let Some(stage) = stage {
            stage["loadouts"].as_array()?.iter().any(|loadout| loadout["amulets"].as_array()
                .is_some_and(|amulets| amulets.iter().any(&matches_amulet)))
        } else {
            profile["initial_amulet_id"].as_u64() == Some(raw_id / 10) && raw_id % 10 == 0
        }))
    };
    if value == Some(0) { return None; }
    let score = value.and_then(|value| value.checked_mul(99)).filter(|score| *score <= MAX_EXACT_SCORE as u64);
    let selling = action.kind == "sell_effect";
    Some((score.map(|score| if selling { -(score as f64) } else { score as f64 }),
        if score.is_none() { "invalid_target_value" } else if selling { "sell_configured_target" } else { "select_configured_target" }))
}

fn map_position(level: &Value, node: &Value) -> Option<(u64, usize, usize)> {
    let level_id = level.as_u64()?;
    let node_id = node.as_u64()?;
    let levels = LEVELS.as_array()?;
    let index = levels.iter().position(|level| level["level"].as_u64() == Some(level_id))?;
    let level = &levels[index];
    Some((level["activity_id"].as_u64()?, index,
        level["nodes"].as_array()?.iter().position(|node| node["node"].as_u64() == Some(node_id))?))
}

fn remake_at_cutoff(state: &Value, _action: &ActionCandidate, _registry: &Value, book: &Value) -> Option<(Option<f64>, &'static str)> {
    let profile = book["profiles"].as_array()?.iter().find(|profile| profile["id"] == book["selected_profile_id"])?;
    let opening = profile["strategy"] == "opening";
    let cutoff = if opening { profile } else {
        match tactical_stage(state, profile) {
            Ok(Some(stage)) => stage,
            Ok(None) => return None,
            Err(reason) => return Some((None, reason)),
        }
    };
    let unmet = (|| {
        let map = &state["provider_state"]["map"];
        let current = map_position(&map["level"], &map["node"])?;
        let deadline = if opening { map_position(&cutoff["cutoff_level"], &cutoff["cutoff_node"])? }
            else { map_position(&cutoff["deadline_level"], &cutoff["deadline_node"])? };
        if current.0 != deadline.0 { return None; }
        if current < deadline { return Some(false); }
        let effects = state["provider_state"]["effect"]["effectList"].as_array()?;
        if effects.iter().any(|effect| effect["id"].as_u64().is_none_or(|id| id == 0) || badge_id(effect).is_none()) { return None; }
        let matches = |effect: &Value, target: &Value| {
            let id = effect["id"].as_u64().unwrap();
            target["id"].as_u64() == Some(id / 10) && target["plus"].as_bool().unwrap_or(false) == (id % 10 == 1)
                && target["badge"].as_u64().is_none_or(|badge| badge_id(effect) == Some(badge))
        };
        if opening {
            let goal = profile["end_count"].as_u64().filter(|goal| *goal > 0)?;
            let value = profile["targets"].as_array()?.iter().try_fold(0u64, |sum, target| {
                let hit = match target["kind"].as_str()? {
                    "amulet" => effects.iter().any(|effect| matches(effect, target)),
                    "badge" => effects.iter().any(|effect| badge_id(effect) == target["id"].as_u64()),
                    _ => return None,
                };
                let value = target.get("value").map_or(Some(1), Value::as_u64).filter(|value| *value > 0)?;
                sum.checked_add(if hit { value } else { 0 })
            })?;
            Some(value < goal)
        } else {
            let met = cutoff["loadouts"].as_array()?.iter().try_fold(false, |met, loadout| {
                let mut targets: Vec<_> = loadout["amulets"].as_array()?.iter().collect();
                // Match specific seals first so wildcard slots cannot consume their only match.
                targets.sort_by_key(|target| target["badge"].is_null());
                let mut remaining: Vec<_> = effects.iter().collect();
                Some(met || targets.iter().all(|target| {
                    if let Some(index) = remaining.iter().position(|effect| matches(effect, target)) {
                        remaining.remove(index);
                        true
                    } else { false }
                }))
            })?;
            Some(!met)
        }
    })();
    match unmet {
        Some(true) => Some((Some(99999.0), "remake_at_cutoff")),
        Some(false) => None,
        None => Some((None, "missing_cutoff_data")),
    }
}

pub(crate) fn tactical_stage<'a>(state: &Value, profile: &'a Value) -> Result<Option<&'a Value>, &'static str> {
    if state["stage"] == 2 { return Ok(None); }
    let missing = "missing_tactical_stage";
    let map = state.pointer("/provider_state/map").filter(|map| map.is_object()).unwrap_or(state);
    let current = map_position(&map["level"], &map["node"]).ok_or(missing)?;
    let mut active = None;
    // shortcut: stage selection follows map starts; use runtime stage state when early transitions are implemented.
    for stage in profile["stages"].as_array().ok_or(missing)? {
        let start = map_position(&stage["start_level"], &stage["start_node"]).ok_or(missing)?;
        if start.0 != current.0 { return Err(missing); }
        if start <= current { active = Some(stage); }
    }
    Ok(active)
}

fn ryan_without_plus(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    let id = action.params["effect"]["id"].as_u64();
    if !matches!(action.kind.as_str(), "select_candidate" | "sell_effect") || !matches!(id, Some(2240 | 2241)) {
        return None;
    }
    let selling = action.kind == "sell_effect";
    let Some(effects) = state["provider_state"]["effect"]["effectList"].as_array() else {
        return Some((None, "missing_effects"));
    };
    let plus_count = effects.iter().filter(|effect| effect["id"].as_u64() == Some(2241)).count();
    // Evaluate retention after selling, so the last PLUS does not suppress its own penalty.
    if plus_count > usize::from(selling && id == Some(2241)) {
        return Some((Some(0.0), if selling { "sell_ryan_plus_remains" } else { "ryan_already_plus" }));
    }
    if effects.iter().any(|effect| !effect["id"].as_u64().is_some_and(|id| id > 0)) {
        return Some((None, "missing_effects"));
    }
    let owned = effects.iter().any(|effect| effect["id"].as_u64() == Some(2240));
    let bonus = 50.0;
    Some((Some(if selling { -bonus } else { bonus }),
        if selling { "sell_ryan" } else if owned { "ryan_not_plus" } else { "ryan_missing" }))
}

fn hp_loss(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    // shortcut: score nominal damage; add server-equivalent mitigation when shield/immune settlement is known.
    let amount = |value: &Value| value.as_i64().or_else(|| value.as_str()?.parse().ok()).filter(|value| *value >= 0);
    let mut loss = if action.cost_resource == "hp" {
        action.cost.as_deref().and_then(|cost| cost.parse::<i64>().ok()).filter(|cost| *cost >= 0)
    } else { Some(0i64) };
    if let Some(cost) = action.params.get("hp_cost") {
        loss = loss.and_then(|loss| loss.checked_add(amount(cost)?));
    }
    let acquired = match action.kind.as_str() {
        "select_candidate" | "buy_effect" => action.params["effect"]["id"].as_u64(),
        "trade" if state["provider_state"]["nodeEvent"]["tradeReward"]["type"] == 1 =>
            state["provider_state"]["nodeEvent"]["tradeReward"]["effectId"].as_u64(),
        _ => None,
    };
    if let Some(damage) = acquired.and_then(|id| ACTION_RULES["acquire_damage"].get(id.to_string())) {
        loss = loss.and_then(|loss| loss.checked_add(amount(damage)?));
    }
    if action.kind == "next_node" && state["stage"] == 3 {
        loss = loss.and_then(|loss| loss.checked_add(battle_entry_damage(state)?));
    }
    if loss == Some(0) { return None; }
    let score = loss.and_then(|loss| loss.checked_mul(2))
        .filter(|penalty| *penalty <= MAX_EXACT_SCORE).map(|penalty| -penalty);
    Some((score.map(|score| score as f64), if score.is_some() { "hp_loss" } else { "invalid_hp_loss" }))
}

fn gamble_outcomes(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    let context = &state["provider_state"];
    let Some(selections) = context["nodeEvent"]["gambleSelections"].as_array().filter(|items| !items.is_empty()) else {
        return Some((None, "missing_gamble_data"));
    };
    let amount = |value: &Value| value.as_u64().or_else(|| value.as_str()?.parse().ok())
        .filter(|value| *value <= MAX_EXACT_SCORE as u64).map(|value| value as f64);
    let hp = amount(&context["character"]["hp"]);
    let max_hp = amount(&context["character"]["maxHp"]);
    let remaining_coin = amount(&context["game"]["coin"]).and_then(|coin| {
        let cost = if action.cost_resource == "coin" {
            let cost = action.cost.as_deref()?.parse::<u64>().ok()?;
            if cost > MAX_EXACT_SCORE as u64 { return None; }
            cost as f64
        } else { 0.0 };
        (coin >= cost).then_some(coin - cost)
    });
    let mut skipped = false;
    let mut total = 0.0;
    for selection in selections {
        let outcome = selection.as_u64().and_then(|id| ACTION_RULES["gamble_selections"].get(id.to_string()));
        let contribution = outcome.and_then(|outcome| {
            let arg = amount(&outcome["args"][0]);
            match outcome["type"].as_u64()? {
                2 => arg,
                16 => Some(-arg?.min(remaining_coin?)),
                24 => Some(-remaining_coin?),
                kind @ (3 | 17) => {
                    let (hp, max_hp) = (hp?, max_hp?);
                    if hp > max_hp { return None; }
                    // shortcut: continuous percentages; use server rounding and mitigation once verified.
                    let change = max_hp * (arg? / 100.0);
                    Some(if kind == 3 { change.min(max_hp - hp) * 2.0 } else { -change.min(hp) * 2.0 })
                }
                1..=27 => Some(0.0),
                _ => None,
            }
        }).filter(|value| value.is_finite() && value.abs() <= MAX_EXACT_SCORE as f64);
        if let Some(value) = contribution { total += value; } else { skipped = true; }
    }
    // The strategy uses equal probability per slot, including repeated selection IDs.
    let score = total / selections.len() as f64;
    Some((Some(score), if skipped { "partial_gamble_data" } else { "gamble_outcomes" }))
}

fn imp_gain(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    let context = &state["provider_state"];
    // shortcut: use confirmed immediate triggers; extend random-pool/removal chains once settlement is verified.
    let acquired = match action.kind.as_str() {
        "select_candidate" | "buy_effect" => action.params["effect"]["id"].as_u64(),
        "trade" if context["nodeEvent"]["tradeReward"]["type"] == 1 =>
            context["nodeEvent"]["tradeReward"]["effectId"].as_u64(),
        _ => None,
    };
    let acquisition = acquired.is_some_and(|id| ACTION_RULES["effects"][id.to_string()]["rarity"] == 5
        || matches!(id, 3490 | 3491 | 4040 | 4041));
    let trade = action.kind == "trade" && context["nodeEvent"]["tradeItems"].as_array().is_some_and(|items|
        items.iter().any(|item| ACTION_RULES["trade_types"][item["id"].to_string()] == 4));
    let gamble = action.kind == "gamble" && context["nodeEvent"]["gambleSelections"].as_array().is_some_and(|selections|
        selections.iter().any(|id| ACTION_RULES["gamble_selections"][id.to_string()]["type"] == 22));
    let battle = action.kind == "next_node" && state["stage"] == 3
        && context["effect"]["effectList"].as_array().is_some_and(|owned|
            owned.iter().any(|effect| matches!(effect["id"].as_u64(), Some(3010 | 3011))));
    if !acquisition && !trade && !gamble {
        if !battle { return None; }
        let next_type = || -> Option<u64> {
            let level = LEVELS.as_array()?.iter().find(|level| level["level"] == context["map"]["level"])?;
            let nodes = level["nodes"].as_array()?;
            let current = nodes.iter().position(|node| node["node"] == context["map"]["node"])?;
            nodes.get(current + 1)?["type"].as_u64()
        };
        match next_type() {
            Some(2 | 4) => {},
            Some(_) => return None,
            None => return Some((None, "missing_imp_outcome")),
        }
    }

    let capacity = || -> Option<bool> {
        let owned = context["effect"]["effectList"].as_array()?;
        let maximum = context["effect"].get("maxEffectVolume").unwrap_or(&context["game"]["maxEffectVolume"]).as_u64()?;
        let mut used = owned.iter().try_fold(0u64, |used, effect|
            used.checked_add(crate::action_provider::volume(effect)?))?;
        if action.kind == "select_candidate" {
            let candidate = &action.params["effect"];
            if owned.iter().any(|effect| !effect["id"].as_u64().is_some_and(|id| id > 0)) { return None; }
            if crate::action_provider::completes_effect_group(candidate, owned) { return None; }
            if let Some(replaced) = crate::action_provider::replaced_effect(candidate, owned) {
                used = used.checked_sub(crate::action_provider::volume(replaced)?)?;
            }
            used = used.checked_add(crate::action_provider::volume(candidate)?)?;
        }
        Some(used >= maximum)
    };
    Some(match capacity() {
        Some(true) => (Some(0.0), "imp_capacity_full"),
        Some(false) => (Some(-15.0), "imp_gain"),
        None => (None, "missing_imp_capacity"),
    })
}

fn badge_id(effect: &Value) -> Option<u64> {
    let effect = effect.as_object()?;
    if let Some(id) = effect.get("badgeId") { return id.as_u64(); }
    match effect.get("badge") {
        None | Some(Value::Null) => Some(0), // Protobuf omits the badge when no seal is attached.
        Some(Value::Object(badge)) if badge.is_empty() => Some(0),
        Some(badge) => badge["id"].as_u64(),
    }
}

fn amulet_price(state: &Value, action: &ActionCandidate, registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    let effect = &action.params["effect"];
    let id = effect["id"].as_u64().filter(|id| *id > 0)?;
    let price_key = if id % 10 == 1 { "plus_sell_price" } else { "sell_price" };
    let metadata_key = if id % 10 == 1 { "plus_metadata" } else { "metadata" };
    let price = registry["amulets"].as_array()
        .and_then(|amulets| amulets.iter().find(|amulet| amulet["id"].as_u64() == Some(id / 10)))
        .and_then(|amulet| if amulet[price_key].is_null() { amulet[metadata_key]["sell_price"].as_u64() }
            else { amulet[price_key].as_u64() });
    if price == Some(0) { return None; }
    let Some((price, badge)) = price.zip(badge_id(effect)) else {
        return Some((None, "missing_amulet_price"));
    };
    let (multiplier, reason) = if action.kind == "sell_effect" {
        (if badge == 600050 { 3 } else { 1 }, "sell_amulet_price")
    } else if badge == 600050 {
        if state["source"] == "qyzz" { (3, "amulet_price_gold_qyzz") } else { (2, "amulet_price_gold") }
    } else { (1, "amulet_price") };
    let score = price.checked_mul(multiplier).filter(|score| *score <= MAX_EXACT_SCORE as u64);
    Some((score.map(|score| score as f64), if score.is_some() { reason } else { "missing_amulet_price" }))
}

fn happiness(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    if !matches!(action.kind.as_str(), "select_candidate" | "sell_effect") { return None; }
    if badge_id(&action.params["effect"]) != Some(600110) { return None; }
    if action.kind == "select_candidate" { return Some((Some(20.0), "select_happiness")); }
    let cost = match state["provider_state"]["shop"].get("goods") {
        None | Some(Value::Null) if state["stage"] != 9 => Some(0),
        Some(Value::Array(goods)) => goods.iter().try_fold(0u64, |total, good| {
            if !matches!(good["goodsId"].as_u64(), Some(101..=103 | 111..=113)) { return Some(total); }
            let sold = good["sold"].as_bool().or_else(|| good["sold"].as_u64().map(|sold| sold != 0))?;
            if sold { return Some(total); }
            let price = good["price"].as_u64().or_else(|| good["price"].as_str()?.parse::<u64>().ok())?;
            total.checked_add(price).filter(|total| *total <= MAX_EXACT_SCORE as u64)
        }),
        _ => None,
    };
    let score = cost.map(|cost| (cost as f64 - 10.0) * 2.0)
        .filter(|score| score.abs() <= MAX_EXACT_SCORE as f64);
    Some((score, if score.is_some() { "sell_happiness" } else { "missing_happiness_pack_cost" }))
}

fn replacement_losses(state: &Value, action: &ActionCandidate, registry: &Value, playbook: &Value) -> Vec<RuleScore> {
    let candidate = &action.params["effect"];
    if !ACTION_RULES["effects"][candidate["id"].to_string()]["upgrade"].as_u64().is_some_and(|id| id > 0) {
        return vec![];
    }
    let Some(owned) = state["provider_state"]["effect"]["effectList"].as_array()
        .filter(|owned| owned.iter().all(|effect| effect["id"].as_u64().is_some_and(|id| id > 0))) else {
        return vec![RuleScore { rule_id: "replacement_amulet".into(), score: None,
            reason: ActionReason { code: "missing_effects".into(), values: json!({}) } }];
    };
    // shortcut: group merges can consume several cards; wait for a known merged-list prediction.
    if crate::action_provider::completes_effect_group(candidate, owned) {
        return vec![RuleScore { rule_id: "replacement_amulet".into(), score: None,
            reason: ActionReason { code: "unknown_replacement_group".into(), values: json!({}) } }];
    }
    let Some(effect) = crate::action_provider::replaced_effect(candidate, owned) else { return vec![]; };
    let mut consumed = action.clone();
    consumed.kind = "sell_effect".into();
    consumed.params = json!({"effect":effect});
    // Consuming a card loses its value without receiving sale proceeds or triggering sale rewards.
    let price = amulet_price(state, &consumed, registry, playbook).map(|(score, reason)| (score.map(|value| -value), reason));
    let happiness = (badge_id(effect) == Some(600110)).then_some((Some(-20.0), "replacement_happiness"));
    [
        ("replacement_amulet", price),
        ("replacement_guidance", guidance(state, &consumed, registry, playbook)),
        ("replacement_happiness", happiness),
        ("replacement_target", configured_target(state, &consumed, registry, playbook)),
    ].into_iter().filter_map(|(id, contribution)| {
        let (score, reason) = contribution?;
        if score == Some(0.0) { return None; }
        Some(RuleScore { rule_id: id.into(), score,
            reason: ActionReason { code: if score.is_some() { id } else { reason }.into(), values: json!({}) } })
    }).collect()
}

fn guidance_count(state: &Value) -> Option<usize> {
    state["provider_state"]["effect"]["effectList"].as_array()
        .and_then(|effects| effects.iter().try_fold(0usize, |count, effect|
            Some(count + usize::from(badge_id(effect)? == 600070))))
}

fn pack_value(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    // Weights run from common to mythic; prices are 3, 6, 9, 12 coins.
    let (mut weights, guaranteed): ([f64; 4], usize) = match action.params["goods_id"].as_u64()? {
        101 | 111 => ([75.0, 22.0, 3.0, 0.0], 0),
        102 | 112 => ([70.0, 25.0, 5.0, 0.0], 1),
        103 | 113 => ([0.0, 82.0, 15.0, 3.0], 2),
        _ => return None,
    };
    let Some(count) = guidance_count(state) else { return Some((None, "missing_guidance_count")); };
    // Dividing the lower weights by 3^G is equivalent and avoids overflowing the upper weights.
    let scale = 3.0_f64.powf(-(count as f64));
    weights[0] *= scale;
    weights[1] *= scale;
    let total: f64 = weights.iter().sum();
    let mut cumulative = 0.0;
    let mut expected = 12.0;
    for (tier, weight) in weights[..3].iter().enumerate() {
        cumulative += weight;
        // One fixed-rarity candidate sets the floor; only the other two are random.
        if tier >= guaranteed { expected -= 3.0 * (cumulative / total).powi(2); }
    }
    Some((Some(expected), "pack_value"))
}

fn gold_pack_bonus(_state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    matches!(action.params["goods_id"].as_u64(), Some(103 | 113))
        .then_some((Some(5.0), "gold_pack_bonus"))
}

fn guidance(state: &Value, action: &ActionCandidate, _registry: &Value, _playbook: &Value) -> Option<(Option<f64>, &'static str)> {
    if !matches!(action.kind.as_str(), "select_candidate" | "sell_effect") { return None; }
    if badge_id(&action.params["effect"]) != Some(600070) { return None; }
    let selling = action.kind == "sell_effect";
    if !selling && state["provider_state"]["effect"]["effectList"].as_array()
        .is_some_and(|owned| crate::action_provider::completes_effect_group(&action.params["effect"], owned)) {
        return Some((None, "unknown_replacement_group"));
    }
    let replaced = state["provider_state"]["effect"]["effectList"].as_array()
        .and_then(|owned| crate::action_provider::replaced_effect(&action.params["effect"], owned));
    let removes_guidance = selling || replaced.is_some_and(|effect| badge_id(effect) == Some(600070));
    let count = guidance_count(state).and_then(|count| count.checked_sub(usize::from(removes_guidance)));
    Some((count.map(|count| {
        const SCORES: [f64; 8] = [30.0, 28.0, 26.0, 20.0, 10.0, 6.0, 3.0, 1.5];
        let score = SCORES.get(count).copied().unwrap_or_else(|| 1.5 * 0.5_f64.powf((count - 7) as f64));
        if selling { -score } else { score }
    }),
        if count.is_none() { "missing_guidance_count" } else if selling { "sell_guidance" } else { "guidance" }))
}

fn battle_entry_damage(state: &Value) -> Option<i64> {
    let context = &state["provider_state"];
    let effects = context["effect"]["effectList"].as_array()?;
    let ids = effects.iter().map(|effect| {
        let id = effect["id"].as_u64()?;
        if matches!(id, 2280 | 2281 | 2320 | 2321) {
            effect["store"][0].as_u64().or_else(|| effect["store"][0].as_str()?.parse().ok())
        } else { Some(id) }
    }).collect::<Option<Vec<_>>>()?;
    if !ids.iter().any(|id| matches!(id, 3620 | 3621)) { return Some(0); }
    let level = LEVELS.as_array()?.iter().find(|level| level["level"] == context["map"]["level"])?;
    let nodes = level["nodes"].as_array()?;
    let current = nodes.iter().position(|node| node["node"] == context["map"]["node"])?;
    let next = nodes.get(current + 1)?;
    if !matches!(next["type"].as_u64()?, 2 | 4) { return Some(0); }
    let hp = context["character"]["hp"].as_i64()
        .or_else(|| context["character"]["hp"].as_str()?.parse().ok())?;
    if hp < 0 { return None; }
    Some(hp.saturating_sub(1).max(0))
}
