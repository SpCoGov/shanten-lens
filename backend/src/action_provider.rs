//! Read-only candidates from the current game snapshot; no scoring or execution.
#[cfg(test)]
mod tests;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use specta::Type;
use std::{cmp::Ordering, collections::HashSet, sync::LazyLock};

pub(crate) static RULES: LazyLock<Value> = LazyLock::new(|| serde_json::from_str::<Value>(include_str!("../assets/action_rules.json")).unwrap()["rules"].clone());
pub(crate) static LEVELS: LazyLock<Value> = LazyLock::new(|| serde_json::from_str::<Value>(include_str!("../assets/levels.json")).unwrap()["levels"].clone());

#[derive(Debug, Clone, Deserialize, Serialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Availability { Available, Unavailable, Unknown }

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ActionReason {
    pub code: String,
    pub values: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ActionCandidate {
    pub id: String,
    pub kind: String,
    pub params: Value,
    pub cost: Option<String>,
    pub cost_resource: String,
    pub availability: Availability,
    pub reasons: Vec<ActionReason>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
pub struct ActionReport {
    pub stage: i64,
    pub interrupt: Option<u64>,
    pub status: String,
    pub actions: Vec<ActionCandidate>,
    pub global_actions: Vec<ActionCandidate>,
    pub issues: Vec<ActionReason>,
}

const SECTIONS: &[&str] = &["state", "game", "character", "effect", "shop", "nodeEvent", "map", "round"];

pub(crate) fn activity_id(state: &Value) -> Option<u64> {
    let levels = LEVELS.as_array()?;
    levels.iter().find(|level| level["level"] == state["level"])
        .or_else(|| levels.last())?["activity_id"].as_u64()
}

// Keep missing fields distinguishable from zero and avoid UI defaults becoming legality evidence.
pub(crate) fn snapshot(target: &mut Map<String, Value>, game: &Value) {
    target.insert("provider_state".into(), Value::Object(SECTIONS.iter().filter_map(|key|
        game.get(*key).map(|value| ((*key).into(), if *key == "round" {
            Value::Object(value.get("tianDora").map(|tiles| ("tianDora".into(), tiles.clone())).into_iter().collect())
        } else { value.clone() }))).collect()));
}

pub(crate) fn update(target: &mut Map<String, Value>, event: &Value) {
    let context = target.entry("provider_state").or_insert_with(|| json!({}));
    let changes = &event["valueChanges"];
    // Node event prices and selections belong to a single map node.
    if ["level", "node"].iter().any(|key| {
        let incoming = &changes["map"][key];
        incoming["dirty"] == true && incoming["value"] != context["map"][key]
    }) {
        context["shop"] = json!({});
        context["nodeEvent"] = json!({});
    }
    for section in SECTIONS {
        if let Some(fields) = changes[section].as_object() {
            if !context[section].is_object() { context[section] = json!({}); }
            for (key, value) in fields {
                if *section == "round" && key != "tianDora" { continue; }
                if value.get("dirty").is_some() {
                    if value["dirty"] == true {
                        // A dirty protobuf scalar can omit its default-valued `value`.
                        if let Some(value) = value.get("value") {
                            context[section][key] = value.clone();
                        } else {
                            context[section].as_object_mut().unwrap().remove(key);
                        }
                    }
                } else {
                    context[section][key] = value.clone();
                }
            }
        }
    }
    if let Some(state) = event.get("state") { context["state"] = state.clone(); }
}

impl ActionCandidate {
    fn new(kind: &str, params: Value) -> Self {
        Self { id: format!("{kind}:{params}"), kind: kind.into(), params, cost: None, cost_resource: "coin".into(),
            availability: Availability::Available, reasons: vec![] }
    }
    fn block(&mut self, code: &str, values: Value) {
        self.availability = Availability::Unavailable;
        self.reasons.push(ActionReason { code: code.into(), values });
    }
    fn unknown(&mut self, field: &str) {
        if self.availability == Availability::Available { self.availability = Availability::Unknown; }
        self.reasons.push(ActionReason {code: "missing_data".into(), values: json!({"field": field})});
    }
    fn price(&mut self, price: &Value, coin: &Value) {
        let Some(price) = integer(price) else { self.unknown("price"); return };
        self.cost = Some(price.to_string());
        match compare_amount(coin, price) {
            Some(Ordering::Less) => self.block("insufficient_coin", json!({"cost":price.to_string(),"coin":coin})),
            None => self.unknown("coin"),
            _ => {}
        }
    }
    fn shop_price(&mut self, price: &Value, context: &Value) {
        let Some(effects) = context["effect"]["effectList"].as_array() else { self.unknown("effectList"); return };
        let mut hp_rate = None;
        for effect in effects {
            let id = effect["id"].as_u64().unwrap_or(0);
            let actual = if matches!(id, 2280 | 2281 | 2320 | 2321) {
                integer(&effect["store"][0]).and_then(|id| u64::try_from(id).ok()).unwrap_or(0)
            } else { id };
            if matches!(actual, 3630 | 3631) { hp_rate = integer(&RULES["shop_hp_rates"][actual.to_string()]); }
        }
        if let Some(rate) = hp_rate {
            self.cost_resource = "hp".into();
            let Some(cost) = integer(price).and_then(|p| p.checked_mul(rate)) else { self.unknown("price"); return };
            // Current payment effects use whole-HP costs (100% / 200%).
            if cost % 100 != 0 { self.unknown("hp_price"); return; }
            let cost = cost / 100;
            self.cost = Some(cost.to_string());
            match compare_amount(&context["character"]["hp"], cost) {
                Some(Ordering::Greater) => {},
                Some(_) => self.block("insufficient_hp", json!({"cost":cost.to_string(),"hp":context["character"]["hp"]})),
                None => self.unknown("hp"),
            }
        } else { self.price(price, &context["game"]["coin"]); }
    }
}

fn integer(value: &Value) -> Option<u128> {
    value.as_u64().map(u128::from).or_else(|| value.as_str()?.parse().ok())
}
fn items(value: &Value) -> &[Value] { value.as_array().map(Vec::as_slice).unwrap_or(&[]) }
fn flag(value: &Value) -> Option<bool> {
    value.as_bool().or_else(|| value.as_u64().map(|value| value != 0))
}

// Compare decimal/scientific money strings exactly without expanding large exponents.
pub(crate) fn compare_amount(value: &Value, required: u128) -> Option<Ordering> {
    let text = value.as_str().map(str::to_owned).or_else(|| value.as_u64().map(|n| n.to_string()))?;
    let (mantissa, exponent) = text.split_once(['e', 'E']).map_or(Some((text.as_str(), 0)), |(m, e)| Some((m, e.parse::<i64>().ok()?)))?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|b| b.is_ascii_digit()) { return None; }
    let combined = format!("{whole}{fraction}");
    let digits = combined.trim_start_matches('0');
    if digits.is_empty() { return Some(0.cmp(&required)); }
    if required == 0 { return Some(Ordering::Greater); }
    let position = (digits.len() as i64).checked_add(exponent)?.checked_sub(fraction.len() as i64)?;
    let rhs = required.to_string();
    let length_cmp = position.cmp(&(rhs.len() as i64));
    if length_cmp != Ordering::Equal { return Some(length_cmp); }
    for (index, digit) in rhs.bytes().enumerate() {
        let cmp = digits.as_bytes().get(index).copied().unwrap_or(b'0').cmp(&digit);
        if cmp != Ordering::Equal { return Some(cmp); }
    }
    Some(if digits.bytes().skip(rhs.len()).any(|b| b != b'0') { Ordering::Greater } else { Ordering::Equal })
}

fn activity(context: &Value) -> u64 {
    activity_id(&context["map"]).unwrap()
}

fn effect_rule(effect: &Value) -> &Value { &RULES["effects"][effect["id"].to_string()] }
pub(crate) fn volume(effect: &Value) -> Option<u64> {
    if let Some(value) = effect["volume"].as_u64() { return Some(value); }
    let badge = effect.get("badgeId").or_else(|| effect.pointer("/badge/id"))?.as_u64()?;
    if badge == 0 { return Some(1); }
    let rule = &RULES["badges"][badge.to_string()];
    if rule.is_null() { None } else { Some(1 + rule["volume"].as_u64().unwrap_or(0)) }
}

pub(crate) fn replaced_effect<'a>(candidate: &Value, owned: &'a [Value]) -> Option<&'a Value> {
    let rule = effect_rule(candidate);
    let upgrade = rule["upgrade"].as_u64().filter(|id| *id > 0)?;
    owned.iter().find(|effect| effect_rule(effect)["upgrade"] == upgrade)
        .or_else(|| owned.iter().find(|effect| effect["id"] == upgrade && rule["duplicate_enabled"] != 1))
}

pub(crate) fn completes_effect_group(candidate: &Value, owned: &[Value]) -> bool {
    let Some(group) = effect_rule(candidate)["effect_group"].as_u64().filter(|group| *group > 0) else { return false; };
    let present: HashSet<u64> = owned.iter().chain([candidate]).filter_map(|e| e["id"].as_u64().map(|id| id / 10)).collect();
    RULES["effects"].as_object().is_some_and(|rules| rules.iter().filter(|(_, rule)| rule["effect_group"] == group)
        .all(|(id, _)| id.parse::<u64>().is_ok_and(|id| present.contains(&(id / 10)))))
}

fn capacity(candidate: &Value, owned: &[Value], maximum: &Value) -> Option<bool> {
    let rule = effect_rule(candidate);
    if rule.is_null() { return None; }
    if completes_effect_group(candidate, owned) { return Some(true); }
    let used: u64 = owned.iter().map(volume).collect::<Option<Vec<_>>>()?.into_iter().sum();
    let needed = volume(candidate)?;
    let max = maximum.as_u64()?;
    if used + needed <= max { return Some(true); }
    if let Some(existing) = replaced_effect(candidate, owned) { return Some(used + needed - volume(existing)? <= max); }
    Some(false)
}

fn check_capacity(action: &mut ActionCandidate, candidate: &Value, context: &Value, owned: Option<&[Value]>) {
    let owned = owned.or_else(|| context["effect"]["effectList"].as_array().map(Vec::as_slice));
    let maximum = context["effect"].get("maxEffectVolume").unwrap_or(&context["game"]["maxEffectVolume"]);
    match owned.and_then(|owned| capacity(candidate, owned, maximum)) {
        Some(true) => {},
        Some(false) => action.block("capacity", json!({"maximum":maximum})),
        None => action.unknown("capacity"),
    }
}

fn sales(actions: &mut Vec<ActionCandidate>, context: &Value) {
    for effect in items(&context["effect"]["effectList"]) {
        let mut action = ActionCandidate::new("sell_effect", json!({"uid":effect["uid"],"effect":effect}));
        if effect["uid"].as_u64().is_none() { action.unknown("uid"); }
        if effect_rule(effect).is_null() { action.unknown("can_sell"); }
        else if effect_rule(effect)["can_sell"] != 1 { action.block("cannot_sell", json!({})); }
        actions.push(action);
    }
}

fn select_candidates(actions: &mut Vec<ActionCandidate>, context: &Value, initial: bool) {
    for (index, candidate) in items(&context["effect"]["packCandidates"]).iter().enumerate() {
        let mut action = ActionCandidate::new("select_candidate", json!({"index":index,"effect":candidate}));
        check_capacity(&mut action, candidate, context, None);
        actions.push(action);
    }
    let mut skip = ActionCandidate::new("skip", json!({}));
    if initial { skip.block("initial_must_choose", json!({})); }
    else { match flag(&context["effect"]["packDisableSkip"]) {
        Some(true) => skip.block("skip_forbidden", json!({})), Some(false) => {}, None => skip.unknown("packDisableSkip"),
    }}
    actions.push(skip);
    if !initial { sales(actions, context); }
}

fn shop(actions: &mut Vec<ActionCandidate>, context: &Value, advanced: bool) {
    let shop = if advanced { &context["nodeEvent"] } else { &context["shop"] };
    let goods_key = if advanced { "advancedShopGoods" } else { "goods" };
    for good in items(&shop[goods_key]) {
        let kind = if !advanced { "buy_pack" } else if good["type"] == 1 { "buy_effect" } else { "buy_rune" };
        let mut action = ActionCandidate::new(kind, json!({"id":good["id"],"goods_id":good["goodsId"],"effect":good["effect"],"rune_id":good["runeStone"]}));
        if good["id"].as_u64().is_none() { action.unknown("id"); }
        match flag(&good["sold"]) { Some(true) => action.block("sold", json!({})), None => action.unknown("sold"), _ => {} }
        action.shop_price(&good["price"], context);
        if advanced && good["type"] == 1 { check_capacity(&mut action, &good["effect"], context, None); }
        else if advanced && good["type"] != 2 { action.unknown("goods_type"); }
        actions.push(action);
    }
    let mut upgrades = items(&shop[if advanced { "advancedShopUpgrade" } else { "upgradeGoods" }]).to_vec();
    if !advanced {
        // Fully upgraded entries can disappear from goods while their buffs remain visible.
        for buff in items(&context["effect"]["shopBuffList"]) {
            if matches!(buff["id"].as_u64(), Some(8002..=8004)) && !upgrades.iter().any(|good| good["upgradeId"] == buff["id"]) {
                upgrades.push(json!({"upgradeId":buff["id"]}));
            }
        }
    }
    for good in &upgrades {
        let mut action = ActionCandidate::new(if advanced { "advanced_upgrade" } else { "shop_upgrade" }, json!({"id":good["id"],"upgrade_id":good["upgradeId"]}));
        if advanced {
            match flag(&good["sold"]) { Some(true) => action.block("sold", json!({})), None => action.unknown("sold"), _ => {} }
        } else {
            let levels: Vec<_> = items(&RULES["shop_upgrades"]).iter().filter(|r| r["id"] == good["upgradeId"]).collect();
            let level = items(&context["effect"]["shopBuffList"]).iter().find(|e| e["id"] == good["upgradeId"])
                .and_then(|e| integer(&e["store"][0]));
            let max = levels.iter().filter_map(|r| integer(&r["level"])).max();
            match (level, max) {
                (Some(l), Some(m)) if l >= m => action.block("maxed", json!({})),
                (None, _) | (_, None) => action.unknown("upgrade_level"),
                _ => {},
            }
            if good["upgradeId"] == 8004 {
                match context["round"]["tianDora"].as_array() {
                    Some(tiles) if tiles.len() >= 27 => action.block("maxed", json!({})),
                    None => action.unknown("tianDora"), _ => {},
                }
            } else if good["upgradeId"] == 8003 {
                let character = items(&RULES["characters"]).iter().find(|r| r["activity_id"] == activity(context) && r["character_id"] == context["character"]["characterId"]);
                if let (Some(character), Some(level)) = (character, level) {
                    let added = levels.iter().find(|r| integer(&r["level"]) == Some(level)).and_then(|r| integer(&r["add_value"])).unwrap_or(0);
                    if integer(&character["init_open_desktop_count"]).unwrap_or(0) + added >= integer(&character["init_desktop_count"]).unwrap_or(u128::MAX) { action.block("maxed", json!({})); }
                } else { action.unknown("characterId"); }
            }
        }
        if !action.reasons.iter().any(|reason| reason.code == "maxed") {
            if good["id"].as_u64().is_none() { action.unknown("id"); }
            action.shop_price(&good["price"], context);
        }
        actions.push(action);
    }
    let mut refresh = ActionCandidate::new("refresh_shop", json!({}));
    refresh.shop_price(&shop[if advanced { "advancedShopRefreshPrice" } else { "refreshPrice" }], context);
    actions.push(refresh);
    actions.push(ActionCandidate::new(if advanced { "skip" } else { "next_node" }, json!({})));
    sales(actions, context);
}

fn forge(actions: &mut Vec<ActionCandidate>, context: &Value) {
    for effect in items(&context["effect"]["effectList"]) {
        let mut action = ActionCandidate::new("forge_effect", json!({"uid":effect["uid"],"effect":effect}));
        let rule = effect_rule(effect);
        let prices = items(&RULES["forge"]);
        let price = |kind: u64, rarity: &Value| prices.iter().find(|r| r["activity_id"] == activity(context) && r["type"] == kind && &r["rarity"] == rarity).and_then(|r| r["price"].as_u64());
        let base = if rule.is_null() { None } else if rule["upgrade"].as_u64().is_some_and(|id| id > 0) { price(1, &rule["rarity"]) } else { Some(0) };
        let badge = effect["badge"]["id"].as_u64().unwrap_or(0);
        let rarity = if badge == 0 { json!(0) } else { RULES["badges"][badge.to_string()]["rarity"].clone() };
        let badge = price(2, &rarity);
        if let (Some(base), Some(badge)) = (base, badge) { action.price(&json!(base + badge), &context["game"]["coin"]); }
        else { action.unknown("forge_price"); }
        if effect["uid"].as_u64().is_none() { action.unknown("uid"); }
        actions.push(action);
    }
    if items(&context["effect"]["effectList"]).is_empty() {
        let mut action = ActionCandidate::new("forge_effect", json!({}));
        if context["effect"]["effectList"].is_array() { action.block("no_amulet", json!({})); }
        else { action.unknown("effectList"); }
        actions.push(action);
    }
    actions.push(ActionCandidate::new("skip", json!({})));
    sales(actions, context);
}

fn trade(actions: &mut Vec<ActionCandidate>, context: &Value) {
    let conditions = items(&context["nodeEvent"]["tradeItems"]);
    let mut base = ActionCandidate::new("trade", json!({"uids":[]}));
    if !context["effect"]["effectList"].is_array() { base.unknown("effectList"); }
    let mut coin = 0u128;
    let mut hp = 0u128;
    let mut required = 0usize;
    let mut demons = 0usize;
    if conditions.is_empty() { base.unknown("tradeItems"); }
    for condition in conditions {
        match RULES["trade_types"][condition["id"].to_string()].as_u64() {
            Some(kind @ (1 | 3)) => {
                if let Some(amount) = integer(&condition["store"][0]) {
                    let sum = if kind == 1 { &mut coin } else { &mut hp };
                    if let Some(value) = sum.checked_add(amount) { *sum = value; } else { base.unknown("trade_cost"); }
                } else { base.unknown("trade_cost"); }
            }
            Some(2) => required += 1,
            Some(4) => demons += 1,
            Some(5) => {}, // Tile penalties impose no additional client-side prerequisite.
            _ => base.unknown("trade_condition"),
        }
    }
    base.price(&json!(coin.to_string()), &context["game"]["coin"]);
    if hp > 0 { match compare_amount(&context["character"]["hp"], hp) {
        Some(Ordering::Greater) => {},
        Some(_) => base.block("insufficient_hp", json!({"cost":hp.to_string(),"hp":context["character"]["hp"]})),
        None => base.unknown("hp"),
    }}
    base.params["hp_cost"] = json!(hp.to_string());
    let owned = items(&context["effect"]["effectList"]);
    // At most eight owned amulets: enumerate distinct sacrifices, not their permutations.
    if owned.len() > 8 { base.unknown("effectList"); actions.push(base); }
    else if required > owned.len() {
        base.block("trade_amulets", json!({"required":required,"owned":owned.len()}));
        actions.push(base);
    } else {
        for mask in 0u16..(1 << owned.len()) {
            if mask.count_ones() as usize != required { continue; }
            let mut action = base.clone();
            let selected: Vec<_> = owned.iter().enumerate().filter(|(i,_)| mask & (1 << i) != 0).map(|(_,e)| e["uid"].clone()).collect();
            if selected.iter().any(|uid| uid.as_u64().is_none()) { action.unknown("uid"); }
            action.params["uids"] = json!(selected);
            action.id = format!("trade:{}", action.params["uids"]);
            let mut remaining: Vec<_> = owned.iter().enumerate().filter(|(i,_)| mask & (1 << i) == 0).map(|(_,e)| e.clone()).collect();
            remaining.extend((0..demons).map(|_| json!({"id":3790,"badgeId":0,"volume":1})));
            let reward = &context["nodeEvent"]["tradeReward"];
            match reward["type"].as_u64() {
                Some(1) => check_capacity(&mut action, &json!({"id":reward["effectId"],"badgeId":reward["badgeId"]}), context, Some(&remaining)),
                Some(2 | 3) => {
                    let used = remaining.iter().map(volume).collect::<Option<Vec<_>>>().map(|v| v.iter().sum::<u64>());
                    let maximum = context["effect"].get("maxEffectVolume").unwrap_or(&context["game"]["maxEffectVolume"]);
                    match (used, maximum.as_u64()) {
                        (Some(used), Some(max)) if used > max => action.block("capacity", json!({"maximum":max})),
                        (None, _) | (_, None) => action.unknown("capacity"), _ => {},
                    }
                }
                _ => action.unknown("tradeReward"),
            }
            actions.push(action);
        }
    }
    actions.push(ActionCandidate::new("skip", json!({})));
    sales(actions, context);
}

pub fn evaluate(state: &Value) -> ActionReport {
    let context = &state["provider_state"];
    let stage = state["stage"].as_i64().unwrap_or(-1);
    let interrupt = context["state"]["interrupt"].as_u64();
    let mut report = ActionReport { stage, interrupt, status: "ready".into(), actions: vec![], global_actions: vec![], issues: vec![] };
    if state["source"] != "packet" {
        report.status = if state["source"] == "qyzz" { "unsupported_source" } else { "waiting_state" }.into();
        return report;
    }
    if stage == 100 || state["ended"] == true {
        report.actions.push(ActionCandidate::new("start_game", json!({})));
        return report;
    }
    if stage <= 0 { report.status = "waiting_state".into(); return report; }
    report.global_actions.push(ActionCandidate::new("remake", json!({})));
    let pack = interrupt == Some(1000) || stage == 2 || stage == 16;
    if interrupt.is_some_and(|value| value != 0 && value != 1000) {
        report.status = "unknown_interrupt".into(); return report;
    }
    if pack {
        select_candidates(&mut report.actions, context, stage == 2);
    } else { match stage {
        1 => for id in 201..=204 { report.actions.push(ActionCandidate::new("select_character", json!({"id":id}))); },
        3 => report.actions.push(ActionCandidate::new("next_node", json!({}))),
        4 => report.status = "transition".into(),
        5..=8 => report.status = "battle_delegated".into(),
        9 | 11 => shop(&mut report.actions, context, stage == 11),
        10 => {
            let mut action = ActionCandidate::new("transport", json!({}));
            let price = items(&RULES["transport"]).iter().find(|r| r["activity_id"] == activity(context)).map(|r| &r["price"]).unwrap_or(&Value::Null);
            action.price(price, &context["game"]["coin"]);
            report.actions.push(action);
            report.actions.push(ActionCandidate::new("skip", json!({})));
        }
        12 => trade(&mut report.actions, context),
        13 => {
            for choice in items(&context["nodeEvent"]["bonfireSelection"]) {
                let mut action = ActionCandidate::new("bonfire", json!({"id":choice}));
                if choice.as_u64().is_none() { action.unknown("bonfireSelection"); }
                report.actions.push(action);
            }
            report.actions.push(ActionCandidate::new("skip", json!({})));
            sales(&mut report.actions, context);
        }
        14 => forge(&mut report.actions, context),
        15 => {
            let mut action = ActionCandidate::new("gamble", json!({}));
            action.price(&context["nodeEvent"]["gamblePrice"], &context["game"]["coin"]);
            report.actions.push(action);
            report.actions.push(ActionCandidate::new("skip", json!({})));
            sales(&mut report.actions, context);
        }
        _ => report.status = "unknown_stage".into(),
    }}
    let mut required = vec![];
    if pack { required.push(("/effect/packCandidates", "packCandidates")); }
    if pack && stage != 2 || matches!(stage, 9 | 11..=16) { required.push(("/effect/effectList", "effectList")); }
    if stage == 9 && !pack { required.extend([("/shop/goods", "goods"), ("/shop/upgradeGoods", "upgradeGoods")]); }
    if stage == 11 && !pack { required.extend([("/nodeEvent/advancedShopGoods", "advancedShopGoods"), ("/nodeEvent/advancedShopUpgrade", "advancedShopUpgrade")]); }
    if stage == 13 && !pack { required.push(("/nodeEvent/bonfireSelection", "bonfireSelection")); }
    for (path, field) in required {
        if context.pointer(path).and_then(Value::as_array).is_none() {
            report.issues.push(ActionReason { code: "missing_data".into(), values: json!({"field":field}) });
        }
    }
    if interrupt.is_none() {
        for action in &mut report.actions { action.unknown("interrupt"); }
    }
    report
}
