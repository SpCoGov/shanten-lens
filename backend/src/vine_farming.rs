//! Ex1+ farming policy; uses the normal action evaluator and Wanxiang planner.
use crate::{action_provider::{ActionCandidate, ActionReport, Availability, ActionReason, LEVELS}, decision::{ActionScore, RuleScore}};
use serde_json::{json, Value};
use std::cmp::Ordering;

pub const STRATEGY: &str = "vine_farming";
const VINE: u64 = 3760;
const KNIGHT: u64 = 356;
pub(crate) const SUPPORTS: &[u64] = &[KNIGHT, 351, 160];
const EXPANSION: u64 = 600160;
const DEMON: u64 = 600310;
const CORE: &[u64] = &[377, 232, 169, 166, 150, 62];

fn items(value: &Value) -> &[Value] { value.as_array().map(Vec::as_slice).unwrap_or(&[]) }
fn id(effect: &Value) -> u64 { effect["id"].as_u64().unwrap_or(0) }
fn badge(effect: &Value) -> u64 { effect.get("badgeId").or_else(|| effect.pointer("/badge/id")).and_then(Value::as_u64).unwrap_or(0) }
pub fn selected(book: &Value) -> bool {
    items(&book["profiles"]).iter().any(|p| p["id"] == book["selected_profile_id"] && p["strategy"] == STRATEGY)
}

pub fn ex_number(game: &Value) -> Option<u64> {
    LEVELS.as_array()?.iter().find(|level| level["level"] == game["level"])?["label"]
        .as_str()?.strip_prefix("Ex")?.parse().ok()
}

pub fn reward_shop(game: &Value) -> bool { ex_number(game).is_some_and(|n| n > 0 && n % 5 == 0) }

pub const REFRESH_LIMITS: &[u64] = &[62,93,140,210,315,473,710,1065];

pub fn valid_options(profile: &Value) -> bool {
    ["reward_refresh_limit","normal_refresh_limit"].iter().all(|key| profile.get(*key).is_none_or(|v|
        v.is_null() || v.as_u64().is_some_and(|n| REFRESH_LIMITS.contains(&n))))
        && profile.get("stop_ex").is_none_or(|v| v.is_null() || v.as_u64().is_some_and(|n| (1..=70).contains(&n)))
        && profile.get("pause_half_hp").is_none_or(Value::is_boolean)
}

pub fn stop_reason(game: &Value, profile: &Value) -> Option<(&'static str, &'static str)> {
    if game["ended"] == true || game["stage"] == 100 { return None; }
    if let Some(target) = profile["stop_ex"].as_u64() {
        if ex_number(game).is_some_and(|n| n > target || (n == target && matches!(game["stage"].as_u64(),Some(9|16)))) {
            return Some(("vine_cutoff","vine.cutoff_reached"));
        }
    } else if first_win_insufficient(game) { return Some(("vine_goal","vine.goal_met")); }
    if game["stage"] == 11 { return Some(("vine_mystery","vine.mystery_pause")); }
    if game["stage"] == 12 && game["trade_reward"]["effectId"].as_u64().is_some_and(|n| n / 10 == VINE / 10) {
        return Some(("vine_trade","vine.trade_pause"));
    }
    let red_dragon = matches!(game["stage"].as_u64(),Some(5..=8))
        && items(&game["boss_buff"]).iter().any(|buff| buff.as_u64() == Some(921));
    if profile["pause_half_hp"] != false && !red_dragon && game["hp"].as_u64().zip(game["max_hp"].as_u64())
        .is_some_and(|(hp,max)| max > 0 && u128::from(hp) * 2 < u128::from(max)) {
        return Some(("vine_low_hp","vine.low_hp"));
    }
    None
}

pub fn eligible(game: &Value) -> bool {
    game["source"] == "packet" && game["ended"] != true && LEVELS.as_array().is_some_and(|levels| levels.iter().any(|level|
        level["level"] == game["level"] && level["label"].as_str().and_then(|s| s.strip_prefix("Ex"))
            .and_then(|s| s.parse::<u64>().ok()).is_some_and(|n| n >= 1)))
}

pub fn lineup_valid(game: &Value) -> bool {
    let owned = items(&game["effect_list"]);
    CORE.iter().all(|family| owned.iter().any(|e| id(e) / 10 == *family && matches!(badge(e), 600070 | 600150)))
        && !owned.iter().any(|e| id(e) == VINE + 1)
        && owned.iter().filter(|e| id(e) == VINE).count() <= 1
}

pub fn support_family(owned: &[Value]) -> Option<u64> {
    owned.iter().map(|effect| id(effect) / 10).find(|family| SUPPORTS.contains(family))
}

fn support_target(state: &Value, profile: &Value) -> u64 {
    profile["support_amulet"].as_u64().filter(|family| SUPPORTS.contains(family))
        .or_else(|| support_family(items(&state["provider_state"]["effect"]["effectList"]))).unwrap_or(KNIGHT)
}

fn recovering(owned: &[Value], family: u64) -> bool {
    !owned.iter().any(|e| (id(e) == family * 10 + 1 && badge(e) == 600070)
        || (id(e) == VINE && badge(e) == EXPANSION))
}

pub fn wanted_pack(state: &Value, action: &ActionCandidate, book: &Value) -> bool {
    let profile = items(&book["profiles"]).iter().find(|p| p["id"] == book["selected_profile_id"]).unwrap_or(&Value::Null);
    matches!(action.params["goods_id"].as_u64(), Some(103 | 113))
        || (recovering(items(&state["provider_state"]["effect"]["effectList"]), support_target(state,profile))
            && matches!(action.params["goods_id"].as_u64(), Some(102 | 112)))
}

// Compare positive decimal/scientific strings without losing low digits or expanding exponents.
pub fn compare_points(left: &str, right: &str) -> Option<Ordering> {
    fn parts(value: &str) -> Option<(String, i64)> {
        let (mantissa, exponent) = value.split_once(['e', 'E']).map_or(Some((value, 0)), |(m,e)| Some((m,e.parse::<i64>().ok()?)))?;
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if whole.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit()) { return None; }
        let joined = format!("{whole}{fraction}");
        let digits = joined.trim_start_matches('0').to_owned();
        let position = (digits.len() as i64).checked_add(exponent)?.checked_sub(fraction.len() as i64)?;
        Some((digits, position))
    }
    let (a, ap) = parts(left)?;
    let (b, bp) = parts(right)?;
    if a.is_empty() || b.is_empty() { return Some((!a.is_empty()).cmp(&(!b.is_empty()))); }
    Some(ap.cmp(&bp).then_with(|| (0..a.len().max(b.len())).map(|i|
        a.as_bytes().get(i).copied().unwrap_or(b'0').cmp(&b.as_bytes().get(i).copied().unwrap_or(b'0')))
        .find(|o| *o != Ordering::Equal).unwrap_or(Ordering::Equal)))
}

pub fn next_target(game: &Value) -> Option<String> {
    let levels = LEVELS.as_array()?;
    let current = levels.iter().find(|level| level["level"] == game["level"])?;
    let next = levels.iter().find(|level| level["level"] == current["next_level"])?;
    let targets = items(&next["nodes"]).iter().find_map(|node| node["target_points"].as_object())?;
    targets.values().filter_map(Value::as_str).try_fold(None::<String>, |best, point| {
        if compare_points(point, "0")? != Ordering::Greater { return None; }
        Some(Some(match best { Some(best) if compare_points(&best, point)? != Ordering::Less => best, _ => point.to_owned() }))
    })?
}

pub fn first_win_insufficient(game: &Value) -> bool {
    let first = &game["first_win"];
    first["level"] == game["level"]
        && first["point"].as_str().zip(next_target(game)).is_some_and(|(point,target)|
            compare_points(point, &target) == Some(Ordering::Less))
}

// A candidate's target value uses the same 99-point multiplier as configured targets.
fn target_value(candidate: &Value, owned: &[Value], family: u64) -> f64 {
    let raw = id(candidate);
    if raw == VINE { return 10.0; }
    if raw / 10 == family && !matches!(badge(candidate), EXPANSION | DEMON) {
        let ready = owned.iter().any(|e| id(e) == family * 10 + 1);
        let improves_seal = badge(candidate) == 600070 || (badge(candidate) == 600150
            && !owned.iter().any(|e| id(e) == family * 10 + 1 && badge(e) == 600150));
        if recovering(owned,family) && (!ready || improves_seal) { return 3.0; }
    }
    if SUPPORTS.contains(&(raw / 10)) && raw / 10 != family && recovering(owned,family)
        && !matches!(badge(candidate),EXPANSION|DEMON) {
        let slot = owned.iter().find(|e| SUPPORTS.contains(&(id(e) / 10)));
        if slot.is_none() || slot.is_some_and(|held| id(held) / 10 == raw / 10
            && (id(held) % 10 == 0 || (badge(held) != 600070 && badge(candidate) == 600070))) { return 1.0; }
    }
    0.0
}

pub fn adjust(state: &Value, report: &ActionReport, actions: &[&ActionCandidate], scores: &mut [ActionScore], profile: &Value) {
    let owned = items(&state["provider_state"]["effect"]["effectList"]);
    let family = support_target(state,profile);
    let candidates: Vec<_> = report.actions.iter().filter(|a| a.kind == "select_candidate"
        && a.availability != Availability::Unknown
        && a.reasons.iter().all(|r| r.code == "capacity")) .collect();
    let vine = candidates.iter().any(|a| id(&a.params["effect"]) == VINE);
    let expansion = candidates.iter().any(|a| id(&a.params["effect"]) == VINE && badge(&a.params["effect"]) == EXPANSION);
    let guidance_support = candidates.iter().any(|a| id(&a.params["effect"]) / 10 == family && badge(&a.params["effect"]) == 600070);
    let held_vine = owned.iter().any(|e| id(e) / 10 == VINE / 10);
    let held_support = owned.iter().any(|e| SUPPORTS.contains(&(id(e) / 10)));
    let target_present = candidates.iter().any(|a| id(&a.params["effect"]) / 10 == family
        && target_value(&a.params["effect"],owned,family) > 0.0);
    let restore = recovering(owned,family);
    let expendable = |effect: &Value| !CORE.contains(&(id(effect) / 10)) && !SUPPORTS.contains(&(id(effect) / 10)) && id(effect) / 10 != 376
        && !matches!(badge(effect), EXPANSION | DEMON);
    let has_spare = owned.iter().any(expendable);
    let recyclable: Vec<_> = actions.iter().zip(scores.iter()).filter(|(action,score)| {
        let effect = &action.params["effect"];
        action.kind == "select_candidate" && expendable(effect)
            && crate::action_provider::RULES["effects"][id(effect).to_string()]["can_sell"] == 1
            && crate::action_provider::replaced_effect(effect,owned).is_none()
            && !crate::action_provider::completes_effect_group(effect,owned)
            && score.total.is_some_and(|total| total > 0.0)
            && score.rules.iter().all(|rule| rule.score.is_some())
            && score.rules.iter().any(|rule| rule.rule_id == "amulet_price" && rule.score.is_some_and(|value| value > 0.0))
            && !score.rules.iter().any(|rule| rule.rule_id == "hp_loss" && rule.score.is_some_and(|value| value < 0.0))
    }).map(|(action,_)|action.id.clone()).collect();
    let blue_available = restore && actions.iter().any(|a| a.kind == "buy_pack" && matches!(a.params["goods_id"].as_u64(), Some(102 | 112)));
    let best_target = candidates.iter().map(|a| target_value(&a.params["effect"], owned,family)).fold(0.0, f64::max) * 99.0;
    let mut allowed = Vec::new();
    for (action, score) in actions.iter().zip(scores.iter_mut()) {
        let effect = &action.params["effect"];
        let (permit, bonus, reason) = match action.kind.as_str() {
            "select_candidate" => {
                let value = target_value(effect, owned,family);
                let target = value > 0.0 && (!vine || id(effect) == VINE)
                    && (!guidance_support || id(effect) / 10 != family || badge(effect) == 600070)
                    && !(id(effect) == VINE && (held_vine || (expansion && held_support)));
                (target || (restore && !vine && recyclable.contains(&action.id)), 99.0 * value, "vine_target")
            },
            "sell_effect" => {
                let sell_support = expansion && held_support && SUPPORTS.contains(&(id(effect) / 10));
                let sell_temporary = SUPPORTS.contains(&(id(effect) / 10)) && id(effect) / 10 != family
                    && !vine && (target_present || owned.iter().any(|e| id(e) / 10 == family));
                let sell_vine = vine && id(effect) == VINE && !(expansion && held_support);
                let sell_spare = restore && expendable(effect);
                (sell_support || sell_temporary || sell_vine || sell_spare,
                    if sell_support || sell_temporary || sell_vine { 990.0 } else { 99.0 }, "vine_make_space")
            },
            "buy_pack" => {
                let tier = action.params["goods_id"].as_u64();
                (!has_spare && if blue_available { matches!(tier, Some(102 | 112)) } else { matches!(tier, Some(103 | 113)) }, 99.0, "vine_pack")
            },
            "refresh_shop" => {
                let limit = profile[if reward_shop(state) {"reward_refresh_limit"} else {"normal_refresh_limit"}].as_u64();
                let enough = action.cost_resource == "coin" && action.cost.as_deref().and_then(|s| s.parse::<u128>().ok())
                    .filter(|cost| limit.is_none_or(|limit| *cost <= limit as u128))
                    .and_then(|cost| cost.checked_add(30)).is_some_and(|needed|
                        crate::action_provider::compare_amount(&state["provider_state"]["game"]["coin"], needed).is_some_and(|o| !o.is_lt()));
                (enough, -best_target, "vine_refresh_budget")
            },
            "bonfire" => (state["stage"] == 13 && action.params["id"] == state["provider_state"]["nodeEvent"]["bonfireSelection"][1], 990.0, "vine_bonfire"),
            "skip" | "next_node" => (true, -best_target, "vine_keep_target"),
            _ => (false, 0.0, "vine_protected"),
        };
        score.rules.push(RuleScore { rule_id: reason.into(), score: Some(bonus), reason: ActionReason { code: reason.into(), values: json!({}) } });
        score.total = score.total.map(|total| total + bonus);
        allowed.push(permit);
    }
    // Strategy restrictions stay below every permitted action, regardless of accumulated economic scores.
    let floor = scores.iter().filter_map(|score| score.total).fold(0.0, f64::min) - 99.0;
    for (score, permit) in scores.iter_mut().zip(allowed) {
        if !permit {
            let penalty = floor - score.total.unwrap_or(0.0);
            score.rules.push(RuleScore { rule_id: "vine_protected".into(), score: Some(penalty), reason: ActionReason { code: "vine_protected".into(), values: json!({}) } });
            score.total = Some(floor);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action_provider, decision};

    fn effect(raw: u64, seal: u64) -> Value {
        json!({"id":raw,"uid":raw,"volume":if seal == EXPANSION {2} else {1},"badgeId":seal})
    }
    fn fixture() -> Value {
        let mut owned: Vec<_> = CORE.iter().map(|family| effect(family * 10 + 1,600070)).collect();
        owned.extend([effect(3561,600070),effect(VINE,0)]);
        json!({"source":"packet","stage":9,"level":14016,"node":1,"provider_state":{
            "state":{"current":9,"interrupt":0},"map":{"level":14016,"node":1},"game":{"coin":"1000"},"character":{"hp":50},
            "effect":{"effectList":owned,"shopBuffList":[],"packCandidates":[],"maxEffectVolume":8,"packDisableSkip":false,"packGoodsId":103},
            "shop":{"goods":[],"upgradeGoods":[],"refreshPrice":140}}})
    }
    fn choose(state: &Value) -> ActionCandidate {
        choose_with(state,&json!({}))
    }
    fn choose_with(state: &Value, options: &Value) -> ActionCandidate {
        let registry: Value = serde_json::from_str(include_str!("../assets/amulets.json")).unwrap();
        let report = action_provider::evaluate(state);
        let mut profile = json!({"id":"vine","strategy":STRATEGY});
        profile.as_object_mut().unwrap().extend(options.as_object().unwrap().clone());
        let scores = decision::evaluate(state, &report, &json!({"amulets":registry["items"]}),
            &json!({"selected_profile_id":"vine","profiles":[profile]}));
        let mut game = state.clone();
        game["action_report"] = json!(report);
        game["decision_report"] = json!(scores);
        decision::first_action(&game).unwrap_or_else(|| panic!("No safe action: {game}"))
    }
    fn candidates(state: &mut Value, values: Value) {
        state["provider_state"]["state"]["interrupt"] = json!(1000);
        state["provider_state"]["effect"]["packCandidates"] = values;
    }
    fn remove(state: &mut Value, raw: u64) {
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().retain(|e| id(e) != raw);
    }
    #[test]
    fn ex17_screenshot_buys_gold_and_never_sells_core_or_remakes() {
        let mut state = fixture();
        state["level"] = json!(14017);
        state["provider_state"]["map"]["level"] = json!(14017);
        state["provider_state"]["game"]["coin"] = json!(1532);
        state["provider_state"]["shop"]["refreshPrice"] = json!(3);
        for (i,effect) in state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().iter_mut().enumerate() {
            effect["badgeId"] = json!(if i < 3 {600150} else if i < 7 {600070} else {0});
        }
        let mut goods = vec![json!({"id":1,"goodsId":103,"price":15,"sold":false})];
        goods.extend((2..=5).map(|id|json!({"id":id,"goodsId":102,"price":10,"sold":false})));
        state["provider_state"]["shop"]["goods"] = json!(goods);
        let chosen = choose(&state);
        assert_eq!(chosen.kind,"buy_pack");
        assert_eq!(chosen.params["goods_id"],103);
        let registry: Value = serde_json::from_str(include_str!("../assets/amulets.json")).unwrap();
        let actions = action_provider::evaluate(&state);
        let report = decision::evaluate(&state,&actions,&json!({"amulets":registry["items"]}),
            &json!({"selected_profile_id":"vine","profiles":[{"id":"vine","strategy":STRATEGY}]}));
        for action in actions.actions.iter().chain(&actions.global_actions).filter(|a| matches!(a.kind.as_str(),"remake"|"sell_effect")) {
            let score = report.actions.iter().find(|s|s.action_id == action.id).unwrap();
            assert!(score.total.unwrap() < 0.0,"{} must not have a positive recommendation",action.id);
            assert_eq!(score.rules.last().unwrap().rule_id,"vine_protected");
        }
        state["provider_state"]["shop"]["goods"][0]["sold"] = json!(true);
        assert_eq!(choose(&state).kind,"refresh_shop");
    }
    #[test]
    fn recovery_uses_102_blue_never_101_green_and_stops_only_at_guidance_plus() {
        let mut state = fixture();
        remove(&mut state,3561);
        state["provider_state"]["shop"]["goods"] = json!([
            {"id":1,"goodsId":101,"price":5,"sold":false},
            {"id":2,"goodsId":102,"price":10,"sold":false},
            {"id":3,"goodsId":103,"price":15,"sold":false}]);
        assert_eq!(choose(&state).params["goods_id"],102);
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3560,600010));
        candidates(&mut state,json!([effect(3560,600150)]));
        state["provider_state"]["effect"]["packGoodsId"] = json!(102);
        assert_eq!(choose(&state).kind,"select_candidate");
        remove(&mut state,3560);
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3561,600150));
        state["provider_state"]["state"]["interrupt"] = json!(0);
        assert_eq!(choose(&state).params["goods_id"],102,"Friend PLUS is transitional, so keep opening blue");
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().last_mut().unwrap()["badgeId"] = json!(600070);
        assert_eq!(choose(&state).params["goods_id"],103);
    }
    #[test]
    fn entry_requires_core_seals_and_never_accepts_plus_vine() {
        let mut game = fixture();
        game["effect_list"] = game["provider_state"]["effect"]["effectList"].clone();
        assert!(lineup_valid(&game));
        game["effect_list"][0]["badgeId"] = json!(0);
        assert!(!lineup_valid(&game));
        game["effect_list"][0]["badgeId"] = json!(600150);
        game["effect_list"][7]["id"] = json!(VINE+1);
        assert!(!lineup_valid(&game));
        game["level"] = json!(14015);
        assert!(eligible(&game));
        game["level"] = json!(14001);
        assert!(eligible(&game));
        game["level"] = json!(13003);
        assert!(!eligible(&game));
    }
    #[test]
    fn discrete_refresh_caps_are_independent_and_always_reserve_current_price_plus_thirty() {
        for (level,limit) in [(14005,473),(14006,210),(14015,473),(14016,210)] {
            for price in [140,210,315,473,710] {
                for reserve in [29,30] {
                    let mut state = fixture();state["level"] = json!(level);
                    state["provider_state"]["shop"]["refreshPrice"] = json!(price);
                    state["provider_state"]["game"]["coin"] = json!(price+reserve);
                    let action = choose_with(&state,&json!({"reward_refresh_limit":473,"normal_refresh_limit":210}));
                    assert_eq!(action.kind,if price <= limit && reserve == 30 {"refresh_shop"} else {"next_node"},"level={level}, price={price}, reserve={reserve}");
                }
            }
        }
        assert!(valid_options(&json!({})));
        for option in [json!({"reward_refresh_limit":1}),json!({"normal_refresh_limit":1598}),json!({"stop_ex":71}),json!({"pause_half_hp":null})] {
            assert!(!valid_options(&option));
        }
    }
    #[test]
    fn early_ex_events_skip_safely_and_choose_the_second_bonfire_option() {
        for stage in [12,13,14,15] {
            let mut state=fixture();state["level"]=json!(14001);state["stage"]=json!(stage);
            state["provider_state"]["state"]["current"]=json!(stage);
            state["provider_state"]["nodeEvent"]=json!({"bonfireSelection":[1,2,3],"gamblePrice":5,"gambleSelections":[]});
            let action=choose(&state);
            assert_eq!(action.kind,if stage==13 {"bonfire"} else {"skip"});
            if stage==13 {assert_eq!(action.params["id"],2);}
        }
        let mut game=fixture();game["stage"]=json!(11);
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_mystery");
        game["stage"]=json!(12);game["trade_reward"]=json!({"type":1,"effectId":3760});
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_trade");
    }
    #[test]
    fn configured_boss_cutoff_overrides_first_win_and_half_hp_uses_temporary_maximum() {
        let mut game=fixture();game["first_win"]=json!({"level":14016,"point":"0"});
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_goal");
        assert!(stop_reason(&game,&json!({"stop_ex":17})).is_none());
        game["level"]=json!(14017);game["stage"]=json!(6);
        assert!(stop_reason(&game,&json!({"stop_ex":17})).is_none());
        game["stage"]=json!(16);
        assert_eq!(stop_reason(&game,&json!({"stop_ex":17})).unwrap().0,"vine_cutoff");
        game["stage"]=json!(6);game["hp"]=json!(20);game["max_hp"]=json!(20);
        assert!(stop_reason(&game,&json!({})).is_none(),"Red dragon's temporary 20 HP is full health");
        game["hp"]=json!(10);
        assert!(stop_reason(&game,&json!({})).is_none(),"Exactly half health must continue");
        game["max_hp"]=json!(21);
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_low_hp","Odd maximum HP must not round the half threshold down");
        game["max_hp"]=json!(20);game["hp"]=json!(9);
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_low_hp");
        assert!(stop_reason(&game,&json!({"pause_half_hp":false})).is_none());
        game["boss_buff"]=json!([921]);game["hp"]=json!(4);
        assert!(stop_reason(&game,&json!({})).is_none(),"Red dragon battle ignores low HP even below half");
        game["stage"]=json!(16);game["max_hp"]=json!(184);
        assert_eq!(stop_reason(&game,&json!({})).unwrap().0,"vine_low_hp","A stale boss buff must not disable protection after battle");
    }
    #[test]
    fn refresh_budget_follows_each_price_plus_two_gold_packs() {
        for price in [1,140,210,473,710,1000] {
            for extra in [29,30,31] {
                let mut state = fixture();
                state["provider_state"]["shop"]["refreshPrice"] = json!(price);
                state["provider_state"]["game"]["coin"] = json!((price+extra).to_string());
                // Unwanted affordable blue/purple packs must not suppress refresh scoring.
                state["provider_state"]["shop"]["goods"] = json!([
                    {"id":1,"goodsId":101,"price":5,"sold":false},{"id":2,"goodsId":102,"price":10,"sold":false}]);
                assert_eq!(choose(&state).kind, if extra < 30 {"next_node"} else {"refresh_shop"}, "price={price}, reserve={extra}");
            }
        }
    }
    #[test]
    fn gold_first_and_blue_only_while_restoring_knight() {
        let mut state = fixture();
        state["provider_state"]["shop"]["goods"] = json!([
            {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]);
        assert_eq!(choose(&state).params["goods_id"],103);
        remove(&mut state,3561);
        assert_eq!(choose(&state).params["goods_id"],102);
        remove(&mut state,VINE);
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(VINE,EXPANSION));
        assert_eq!(choose(&state).params["goods_id"],103,"Keep looking for replacement vine before restoring Knight");
    }
    #[test]
    fn captured_ex15_black_hole_uses_family_160_and_protects_uid_204() {
        let mut state: Value = serde_json::from_str(include_str!("../tests/fixtures/vine-ex15-black-hole.json")).unwrap();
        assert!(lineup_valid(&state));
        assert_eq!(support_family(items(&state["effect_list"])),Some(160));
        state["provider_state"]["shop"]["goods"] = json!([
            {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]);
        assert_eq!(choose(&state).params["goods_id"],103,"Never sell black hole or open blue to look for Knight");
        candidates(&mut state,json!([effect(3560,600070),effect(3510,600070)]));
        assert_eq!(choose(&state).kind,"skip","Neither other support may replace the handed-over black hole");
        candidates(&mut state,json!([effect(VINE,0)]));
        assert_eq!(choose(&state).params["effect"]["id"],VINE);
        candidates(&mut state,json!([effect(VINE,EXPANSION)]));
        let action = choose(&state);
        assert_eq!(action.kind,"sell_effect");
        assert_eq!(action.params["effect"]["id"],1601);
        assert_eq!(action.params["uid"],204,"UID is only the server sale argument, not the support family");
    }

    #[test]
    fn handed_over_supports_are_protected_and_restore_only_the_same_guidance_plus() {
        for family in SUPPORTS {
            let mut state = fixture();
            state["provider_state"]["effect"]["effectList"][6] = effect(family * 10 + 1,600070);
            state["provider_state"]["shop"]["goods"] = json!([
                {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]);
            let options = json!({"support_amulet":family});
            assert_eq!(support_family(items(&state["provider_state"]["effect"]["effectList"])),Some(*family));
            assert_eq!(choose_with(&state,&options).params["goods_id"],103,"A complete support never triggers blue recovery");
            for other in SUPPORTS {
                candidates(&mut state,json!([effect(other * 10,600070),effect(other * 10,600150)]));
                assert_eq!(choose_with(&state,&options).kind,"skip","Never replace a formed support for another family or seal");
            }
            candidates(&mut state,json!([effect(VINE,EXPANSION)]));
            assert_eq!(choose_with(&state,&options).params["effect"]["id"],family * 10 + 1);
            remove(&mut state,family * 10 + 1);
            assert_eq!(choose_with(&state,&options).params["effect"]["id"],VINE);
            remove(&mut state,VINE);
            assert_eq!(choose_with(&state,&options).kind,"select_candidate");
            state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(VINE,EXPANSION));
            state["provider_state"]["state"]["interrupt"] = json!(0);
            assert_eq!(choose_with(&state,&options).params["goods_id"],103,"Replace expansion before recovering the support");
            candidates(&mut state,json!([effect(VINE,0)]));
            assert_eq!(choose_with(&state,&options).kind,"sell_effect");
            remove(&mut state,VINE);
            assert_eq!(choose_with(&state,&options).kind,"select_candidate");
            state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(VINE,0));
            state["provider_state"]["state"]["interrupt"] = json!(0);
            assert_eq!(choose_with(&state,&options).params["goods_id"],102,"Recover the handed-over family even while it is absent");
            for other in SUPPORTS.iter().filter(|other| *other != family) {
                candidates(&mut state,json!([effect(other * 10,600070)]));
                assert_eq!(choose_with(&state,&options).kind,"select_candidate","A temporary support can fill the empty slot without changing the target");
            }
            for bad in [EXPANSION,DEMON] {
                candidates(&mut state,json!([effect(family * 10,bad)]));
                assert_eq!(choose_with(&state,&options).kind,"skip","Reject forbidden seals on the first copy");
            }
            candidates(&mut state,json!([effect(family * 10,600150)]));
            assert_eq!(choose_with(&state,&options).kind,"select_candidate");
            state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(family * 10,600150));
            for bad in [EXPANSION,DEMON] {
                candidates(&mut state,json!([effect(family * 10,bad)]));
                assert_eq!(choose_with(&state,&options).kind,"skip","Reject forbidden seals on the second copy");
            }
            candidates(&mut state,json!([effect(family * 10,0)]));
            assert_eq!(choose_with(&state,&options).kind,"select_candidate","Second ordinary copy can still make PLUS");
            remove(&mut state,family * 10);
            state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(family * 10 + 1,600150));
            state["provider_state"]["state"]["interrupt"] = json!(0);
            assert_eq!(choose_with(&state,&options).params["goods_id"],102,"Friend PLUS remains transitional for every support");
            for bad in [0,600150,EXPANSION,DEMON] {
                candidates(&mut state,json!([effect(family * 10,bad)]));
                assert_eq!(choose_with(&state,&options).kind,"skip");
            }
            candidates(&mut state,json!([effect(family * 10,600150),effect(family * 10,600070)]));
            assert_eq!(choose_with(&state,&options).params["index"],1);
            state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().last_mut().unwrap()["badgeId"] = json!(600070);
            state["provider_state"]["state"]["interrupt"] = json!(0);
            assert_eq!(choose_with(&state,&options).params["goods_id"],103,"Only the original family's guidance PLUS ends recovery");
        }
    }
    #[test]
    fn ordinary_vine_replaces_without_merging_and_wins_over_knight() {
        let mut state = fixture();
        candidates(&mut state,json!([effect(3561,600150),effect(VINE,600070)]));
        let action = choose(&state);
        assert_eq!(action.kind,"sell_effect");
        assert_eq!(action.params["effect"]["id"],VINE);
        remove(&mut state,VINE);
        assert_eq!(choose(&state).params["effect"]["id"],VINE);
        candidates(&mut state,json!([effect(VINE+1,600070)]));
        assert_eq!(choose(&state).kind,"skip");
    }
    #[test]
    fn expansion_cycle_sells_knight_then_vine_and_restores_it_after_replacement() {
        let mut state = fixture();
        candidates(&mut state,json!([effect(VINE,EXPANSION)]));
        assert_eq!(choose(&state).params["effect"]["id"],3561);
        remove(&mut state,3561);
        assert_eq!(choose(&state).params["effect"]["id"],VINE);
        remove(&mut state,VINE);
        assert_eq!(choose(&state).kind,"select_candidate");
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(VINE,EXPANSION));
        candidates(&mut state,json!([effect(VINE,600070)]));
        assert_eq!(choose(&state).kind,"sell_effect");
        remove(&mut state,VINE);
        assert_eq!(choose(&state).kind,"select_candidate");
    }
    #[test]
    fn knight_recovery_accepts_both_copies_with_any_seal_except_expansion_or_demon() {
        for first in [false,true] {
            for seal in [0,600010,600070,600150,600180,EXPANSION,DEMON] {
                let mut state = fixture();
                remove(&mut state,3561);
                if !first { state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3560,600010)); }
                candidates(&mut state,json!([effect(3560,seal)]));
                assert_eq!(choose(&state).kind,if matches!(seal,EXPANSION|DEMON) {"skip"} else {"select_candidate"},"first={first} seal={seal}");
            }
        }
    }
    #[test]
    fn formed_guidance_plus_skips_irrelevant_gold_and_never_changes_seal() {
        for seal in [0,600010,600070,600150,EXPANSION,DEMON] {
            let mut state = fixture();
            candidates(&mut state,json!([effect(3560,seal),effect(3390,600070)]));
            assert_eq!(choose(&state).kind,"skip","Never replace a formed guidance PLUS Knight: seal={seal}");
        }
    }
    #[test]
    fn echo_vine_uses_normal_replacement_and_never_causes_a_special_stop() {
        let mut state = fixture();
        candidates(&mut state,json!([effect(VINE,600180)]));
        state["candidate_effect_list"] = json!([effect(VINE,600180)]);
        assert!(stop_reason(&state,&json!({})).is_none());
        assert_eq!(choose(&state).kind,"sell_effect");
        assert_eq!(choose(&state).params["effect"]["id"],VINE);
        remove(&mut state,VINE);
        assert_eq!(choose(&state).kind,"select_candidate");
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(VINE,600180));
        state["effect_list"] = state["provider_state"]["effect"]["effectList"].clone();
        assert!(lineup_valid(&state));
        assert!(stop_reason(&state,&json!({})).is_none());
        candidates(&mut state,json!([effect(VINE,0)]));
        assert_eq!(choose(&state).kind,"sell_effect","Sell the held Echo vine before taking the next ordinary vine");
        assert_eq!(choose(&state).params["effect"]["badgeId"],600180);
        candidates(&mut state,json!([effect(VINE+1,600180)]));
        assert_eq!(choose(&state).kind,"skip","Echo does not allow PLUS vines");
    }
    #[test]
    fn restored_plus_keeps_opening_blue_until_guidance_seal() {
        for held_seal in [0,600010,600180,600300] {
            for target_seal in [600070,600150] {
                let mut state = fixture();
                state["provider_state"]["effect"]["effectList"][6]["badgeId"] = json!(held_seal);
                state["provider_state"]["shop"]["goods"] = json!([
                    {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]);
                assert_eq!(choose(&state).params["goods_id"],102);
                candidates(&mut state,json!([effect(3560,600010)]));
                assert_eq!(choose(&state).kind,"skip","Another unsuitable seal does not improve the restored PLUS");
                candidates(&mut state,json!([effect(3560,target_seal)]));
                assert_eq!(choose(&state).kind,"select_candidate");
                state["provider_state"]["effect"]["effectList"][6]["badgeId"] = json!(target_seal);
                state["provider_state"]["state"]["interrupt"] = json!(0);
                assert_eq!(choose(&state).params["goods_id"],if target_seal == 600070 {103} else {102},
                    "Only guidance PLUS completes recovery");
            }
        }
    }
    #[test]
    fn ex37_friend_is_transitional_and_guidance_never_downgrades() {
        let mut state = fixture();
        state["level"] = json!(14037);
        state["provider_state"]["effect"]["effectList"][6] = json!({"id":3561,"uid":255,"volume":1});
        state["provider_state"]["effect"]["packGoodsId"] = json!(113);
        candidates(&mut state,json!([effect(1660,EXPANSION),effect(3560,600150),effect(3460,600190)]));
        assert_eq!(choose(&state).params["index"],1,"Ex37 reward can improve the unsealed PLUS to friend");
        state["provider_state"]["effect"]["effectList"][6]["badge"] = json!({"id":600150});
        state["provider_state"]["state"]["interrupt"] = json!(0);
        state["provider_state"]["shop"]["goods"] = json!([
            {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]);
        assert_eq!(choose(&state).params["goods_id"],102,"Friend PLUS must keep recovering");
        for seal in [0,600010,600150,EXPANSION,DEMON] {
            candidates(&mut state,json!([effect(3560,seal)]));
            assert_eq!(choose(&state).kind,"skip","Do not replace friend with an equal or worse seal: {seal}");
        }
        candidates(&mut state,json!([effect(3560,600150),effect(3560,600070)]));
        assert_eq!(choose(&state).params["index"],1,"Take guidance over friend in the same pack");
        state["provider_state"]["effect"]["effectList"][6]["badge"]["id"] = json!(600070);
        candidates(&mut state,json!([effect(3560,600150)]));
        assert_eq!(choose(&state).kind,"skip","Never downgrade guidance to friend");
        state["provider_state"]["state"]["interrupt"] = json!(0);
        assert_eq!(choose(&state).params["goods_id"],103,"Guidance PLUS completes recovery");
    }
    #[test]
    fn recovery_recycles_gold_keeps_temporary_until_target_appears_and_never_sells_core() {
        let mut state = fixture();
        remove(&mut state,3561);
        let options=json!({"support_amulet":160});
        for pack in [102,103,112,113] {
            state["provider_state"]["effect"]["packGoodsId"]=json!(pack);
            candidates(&mut state,json!([effect(3390,0)]));
            assert_eq!(choose_with(&state,&options).kind,"select_candidate","Recycle safe positive-value cards in pack {pack}");
        }
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3390,0));
        state["provider_state"]["state"]["interrupt"]=json!(0);
        assert_eq!(choose_with(&state,&options).params["effect"]["id"],3390);
        remove(&mut state,3390);
        candidates(&mut state,json!([effect(3390,0),effect(3560,0)]));
        assert_eq!(choose_with(&state,&options).params["effect"]["id"],3560,"Prefer a useful temporary over recycling");
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3560,0));
        state["provider_state"]["state"]["interrupt"]=json!(0);
        state["provider_state"]["shop"]["goods"]=json!([]);
        state["provider_state"]["game"]["coin"]=json!(10);
        assert_eq!(choose_with(&state,&options).kind,"next_node","Carry the temporary into battle when the refresh budget is exhausted");
        candidates(&mut state,json!([effect(3390,0)]));
        assert_eq!(choose_with(&state,&options).kind,"skip","Do not sacrifice a full temporary slot for recycling");
        candidates(&mut state,json!([effect(3510,600070)]));
        assert_eq!(choose_with(&state,&options).kind,"skip","Do not churn temporary support families");
        candidates(&mut state,json!([effect(3560,600150)]));
        assert_eq!(choose_with(&state,&options).kind,"select_candidate","Can upgrade the same temporary family to PLUS");
        candidates(&mut state,json!([effect(1600,600070)]));
        let sale=choose_with(&state,&options);
        assert_eq!(sale.kind,"sell_effect");assert_eq!(sale.params["effect"]["id"],3560);
        remove(&mut state,3560);
        assert_eq!(choose_with(&state,&options).params["effect"]["id"],1600);
        candidates(&mut state,json!([effect(VINE,0),effect(3560,600070)]));
        assert_eq!(choose_with(&state,&options).params["effect"]["id"],VINE,"Vine still outranks temporary support");
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3561,600070));
        candidates(&mut state,json!([effect(VINE,EXPANSION)]));
        assert_eq!(choose_with(&state,&options).params["effect"]["id"],3561,"Expansion frees the temporary slot too");
        let mut formed=fixture();candidates(&mut formed,json!([effect(3390,0)]));
        assert_eq!(choose(&formed).kind,"skip","A complete lineup still skips non-target gold cards");
        remove(&mut state,3561);
        candidates(&mut state,json!([effect(1660,600070)]));
        assert_eq!(choose_with(&state,&options).kind,"skip","Never recycle by merging or changing a core card");
        candidates(&mut state,json!([effect(3390,EXPANSION),effect(3390,DEMON)]));
        assert_eq!(choose_with(&state,&options).kind,"skip");
        candidates(&mut state,json!([effect(3550,600050)]));
        assert_eq!(choose_with(&state,&options).kind,"skip","Do not spend health just to recycle a high-resale card");
    }

    #[test]
    fn blue_filler_is_bought_then_sold_without_selling_core() {
        let mut state = fixture();
        remove(&mut state,3561);
        state["provider_state"]["effect"]["packGoodsId"] = json!(102);
        candidates(&mut state,json!([effect(3390,0)]));
        assert_eq!(choose(&state).kind,"select_candidate");
        state["provider_state"]["state"]["interrupt"] = json!(0);
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(effect(3390,0));
        let action = choose(&state);
        assert_eq!(action.kind,"sell_effect");
        assert_eq!(action.params["effect"]["id"],3390);
    }
    #[test]
    fn point_comparison_is_exact_and_only_insufficient_first_win_stops_current_level() {
        for (a,b,expected) in [("1E64","1E65",Ordering::Less),("1.0000000000000000001E64","1E64",Ordering::Greater),
            ("10E63","1E64",Ordering::Equal),("0E999","0",Ordering::Equal),("0.001","0.01",Ordering::Less)] {
            assert_eq!(compare_points(a,b),Some(expected));
        }
        for invalid in ["NaN","-1","1.2.3","1e999999999999999999999999",""] { assert_eq!(compare_points(invalid,"1"),None); }
        let mut game = fixture();
        assert!(eligible(&game));
        let threshold = next_target(&game).unwrap();
        assert_eq!(compare_points(&threshold,"4.168E22"),Some(Ordering::Equal),"Use the strongest possible next boss, not the weakest");
        assert!(!first_win_insufficient(&game));
        game["first_win"] = json!({"level":game["level"],"node":game["node"],"point":threshold});
        assert!(!first_win_insufficient(&game),"Equal points can clear the next boss");
        game["first_win"]["point"] = json!("0");
        game["point"] = json!("1E100");
        assert!(first_win_insufficient(&game),"Later accumulated points cannot replace the first win");
        game["first_win"]["point"] = json!("4.167999999999999999E22");
        assert!(first_win_insufficient(&game));
        game["first_win"]["point"] = json!("1E100");
        assert!(!first_win_insufficient(&game));
        for (level,point) in [(14016,"2.6770E38"),(14017,"2.5952E40"),(14018,"1.1213E41")] {
            game["level"] = json!(level);
            game["first_win"] = json!({"level":level,"node":2,"point":point});
            assert!(!first_win_insufficient(&game),"Observed Ex16–18 must continue");
        }
        game["first_win"]["point"] = json!("0");
        game["first_win"]["level"] = json!(14015);
        assert!(!first_win_insufficient(&game));
    }
}
