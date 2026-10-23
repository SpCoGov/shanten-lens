use super::*;

fn state(stage: i64) -> Value {
    json!({"source":"packet","stage":stage,"ended":false,"provider_state":{
        "state":{"current":stage,"interrupt":0}, "game":{"coin":"20"},
        "character":{"characterId":201,"hp":50},"map":{"level":13001,"node":2},
        "effect":{"effectList":[],"maxEffectVolume":8,"packDisableSkip":0,"shopBuffList":[]},
        "shop":{"goods":[],"upgradeGoods":[],"refreshPrice":1},"nodeEvent":{}
    }})
}
fn effect(id: u64, uid: u64) -> Value { json!({"id":id,"uid":uid,"volume":1,"badge":{"id":0}}) }
fn one(report: &ActionReport, kind: &str) -> ActionCandidate { report.actions.iter().find(|a| a.kind == kind).unwrap().clone() }

#[test]
fn documented_stage_boundaries_and_global_actions() {
    let roles = evaluate(&state(1));
    assert_eq!(roles.actions.len(), 4);
    assert_eq!(roles.actions[0].params["id"], 201);
    assert_eq!(roles.actions[3].params["id"], 204);
    assert!(roles.actions.iter().all(|a| a.availability == Availability::Available));
    assert_eq!(roles.global_actions[0].kind, "remake");
    assert_eq!(evaluate(&state(3)).actions[0].kind, "next_node");
    for stage in 5..=8 {
        let report = evaluate(&state(stage));
        assert_eq!(report.status, "battle_delegated");
        assert!(report.actions.is_empty());
    }
    let ended = evaluate(&state(100));
    assert_eq!(ended.actions.len(), 1);
    assert_eq!(ended.actions[0].kind, "start_game");
    assert!(ended.global_actions.is_empty());
    let mut local = state(9); local["source"] = json!("qyzz");
    assert_eq!(evaluate(&local).status, "unsupported_source");
    assert!(evaluate(&json!({})).actions.is_empty());
}

#[test]
fn shop_sold_affordability_maximum_and_unsellable() {
    let mut s = state(9);
    let c = &mut s["provider_state"];
    c["shop"] = json!({"goods":[{"id":1,"goodsId":101,"price":5,"sold":true},{"id":2,"goodsId":102,"price":21,"sold":false}],
        "upgradeGoods":[{"id":1,"upgradeId":8002,"price":10,"level":9}],"refreshPrice":21});
    c["effect"]["shopBuffList"] = json!([{"id":8002,"store":["9"]}]);
    c["effect"]["effectList"] = json!([effect(3790,1),effect(3020,2)]);
    let report = evaluate(&s);
    assert_eq!(report.actions.len(), 7);
    assert_eq!(report.actions[0].reasons[0].code, "sold");
    assert_eq!(report.actions[1].reasons[0].code, "insufficient_coin");
    assert_eq!(one(&report,"shop_upgrade").reasons[0].code,"maxed");
    assert_eq!(one(&report,"refresh_shop").availability, Availability::Unavailable);
    assert_eq!(one(&report,"sell_effect").reasons[0].code, "cannot_sell");
    assert_eq!(report.actions.last().unwrap().availability, Availability::Available);
    assert!(!report.actions.iter().any(|a| a.kind == "sort_effect"));
    s["provider_state"]["shop"]["upgradeGoods"] = json!([]);
    let maxed = one(&evaluate(&s), "shop_upgrade");
    assert_eq!(maxed.availability, Availability::Unavailable);
    assert_eq!(maxed.reasons.len(), 1);
    assert_eq!(maxed.reasons[0].code, "maxed");
    s["provider_state"]["effect"]["shopBuffList"] = json!([{"id":8004,"store":["1"]}]);
    s["provider_state"]["shop"]["upgradeGoods"] = json!([{"id":1,"upgradeId":8004,"price":1}]);
    assert_eq!(one(&evaluate(&s), "shop_upgrade").availability, Availability::Unknown);
    s["provider_state"]["round"] = json!({"tianDora":vec!["1m";27]});
    assert_eq!(one(&evaluate(&s), "shop_upgrade").reasons[0].code, "maxed");
}

#[test]
fn pack_interrupt_hides_shop_and_checks_merging_and_skip() {
    let mut s = state(9);
    let c = &mut s["provider_state"];
    c["state"]["interrupt"] = json!(1000);
    c["effect"]["maxEffectVolume"] = json!(1);
    c["effect"]["effectList"] = json!([effect(3020,1)]);
    c["effect"]["packCandidates"] = json!([{"id":3020,"badgeId":0},{"id":3030,"badgeId":0}]);
    let report = evaluate(&s);
    assert_eq!(report.actions[0].availability, Availability::Available);
    assert_eq!(report.actions[1].availability, Availability::Unavailable);
    assert_eq!(report.actions[1].reasons[0].code,"capacity");
    assert!(!report.actions.iter().any(|a| a.kind == "refresh_shop" || a.kind == "buy_pack"));
    s["stage"] = json!(2);
    let report = evaluate(&s);
    assert_eq!(one(&report,"skip").reasons[0].code,"initial_must_choose");
    assert!(!report.actions.iter().any(|a| a.kind == "sell_effect"));
    s["stage"] = json!(16);
    s["provider_state"]["effect"]["packDisableSkip"] = json!(1);
    assert_eq!(one(&evaluate(&s),"skip").reasons[0].code,"skip_forbidden");
}

#[test]
fn exact_money_and_hp_payment() {
    for (value, required, expected) in [("9007199254740993",9007199254740992,Ordering::Greater),("1e100000",20,Ordering::Greater),
        ("2.000e1",20,Ordering::Equal),("1.99999999999999999999e1",20,Ordering::Less),("20.00001",20,Ordering::Greater),("0e100",0,Ordering::Equal)] {
        assert_eq!(compare_amount(&json!(value),required),Some(expected));
    }
    assert_eq!(compare_amount(&json!("NaN"),1),None);
    let mut s = state(9);
    s["provider_state"]["effect"]["effectList"] = json!([effect(3630,1)]);
    s["provider_state"]["game"]["coin"] = json!("0");
    s["provider_state"]["shop"]["refreshPrice"] = json!(25);
    let action = one(&evaluate(&s),"refresh_shop");
    assert_eq!(action.cost_resource,"hp");
    assert_eq!(action.cost.as_deref(),Some("50"));
    assert_eq!(action.reasons[0].code,"insufficient_hp");
    s["provider_state"]["character"]["hp"] = json!(51);
    assert_eq!(one(&evaluate(&s),"refresh_shop").availability,Availability::Available);
}

#[test]
fn trade_requires_all_conditions_and_distinct_sacrifices() {
    let mut s = state(12);
    s["provider_state"]["effect"]["maxEffectVolume"] = json!(1);
    s["provider_state"]["nodeEvent"] = json!({"tradeItems":[{"id":2,"store":[20]},{"id":6,"store":[1]}],
        "tradeReward":{"id":6,"effectId":2081,"badgeId":0,"type":1}});
    let trade = one(&evaluate(&s),"trade");
    assert_eq!(trade.availability,Availability::Unavailable);
    assert_eq!(trade.reasons[0].code,"capacity");
    s["provider_state"]["effect"]["maxEffectVolume"] = json!(8);
    assert_eq!(one(&evaluate(&s),"trade").availability,Availability::Available);
    s["provider_state"]["game"]["coin"] = json!("19");
    assert_eq!(one(&evaluate(&s),"trade").reasons[0].code,"insufficient_coin");
    s["provider_state"]["game"]["coin"] = json!("20");
    s["provider_state"]["nodeEvent"]["tradeItems"] = json!([{"id":3,"store":[1]},{"id":9,"store":[1]}]);
    s["provider_state"]["effect"]["effectList"] = json!([effect(3020,1),effect(3030,2),effect(3040,3)]);
    let report = evaluate(&s);
    let trades: Vec<_> = report.actions.iter().filter(|a| a.kind == "trade").collect();
    assert_eq!(trades.len(),3);
    assert!(trades.iter().all(|a| a.availability == Availability::Available));
    assert_eq!(trades[0].params["uids"],json!([1,2]));
}

#[test]
fn events_and_missing_data_are_not_free_actions() {
    let mut s = state(11);
    s["provider_state"]["nodeEvent"] = json!({"advancedShopGoods":[{"id":3,"type":2,"runeStone":7,"price":2,"sold":1}],
        "advancedShopUpgrade":[{"id":2,"upgradeId":1,"price":25,"sold":0}],"advancedShopRefreshPrice":1});
    assert_eq!(one(&evaluate(&s),"buy_rune").reasons[0].code,"sold");
    assert_eq!(one(&evaluate(&s),"advanced_upgrade").reasons[0].code,"insufficient_coin");
    s = state(13);
    s["provider_state"]["nodeEvent"]["bonfireSelection"] = json!([1,2,7]);
    assert_eq!(evaluate(&s).actions.len(),4);
    s = state(14);
    s["provider_state"]["effect"]["effectList"] = json!([effect(3020,1),effect(3021,2)]);
    assert_eq!(one(&evaluate(&s),"forge_effect").cost.as_deref(),Some("6"));
    assert_eq!(evaluate(&s).actions[1].cost.as_deref(),Some("1"));
    s = state(10);
    assert_eq!(one(&evaluate(&s),"transport").reasons[0].code,"insufficient_coin");
    s = state(15);
    assert_eq!(one(&evaluate(&s),"gamble").availability,Availability::Unknown);
    s["provider_state"]["nodeEvent"]["gamblePrice"] = json!(0);
    assert_eq!(one(&evaluate(&s),"gamble").availability,Availability::Available);
}

#[test]
fn dirty_updates_clear_stale_node_prices_and_keep_false_dirty_values() {
    let mut target = Map::new();
    snapshot(&mut target,&state(9)["provider_state"]);
    update(&mut target,&json!({"state":{"current":9,"interrupt":0},"valueChanges":{"shop":{"refreshPrice":{"dirty":false,"value":999}}}}));
    assert_eq!(target["provider_state"]["shop"]["refreshPrice"],1);
    update(&mut target,&json!({"valueChanges":{"map":{"node":{"dirty":true,"value":3}},"nodeEvent":{"gamblePrice":{"dirty":true,"value":4}}}}));
    assert!(target["provider_state"]["shop"]["refreshPrice"].is_null());
    assert_eq!(target["provider_state"]["nodeEvent"]["gamblePrice"],4);
    snapshot(&mut target,&json!({"state":{"current":1,"interrupt":0}}));
    assert!(target["provider_state"]["nodeEvent"].is_null());
}
