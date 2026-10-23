use serde_json::{json, Value};
use shanten_backend::{action_provider::{ActionCandidate, ActionReport, Availability}, decision};

fn evaluate_with_registry(state: &Value, actions: &ActionReport, registry: &Value) -> decision::DecisionReport {
    decision::evaluate(state, actions, registry, &Value::Null)
}

fn evaluate(state: &Value, actions: &ActionReport) -> decision::DecisionReport {
    // Zero-price fixtures isolate the existing rules; price and combined totals are tested below.
    let amulets: Vec<_> = actions.actions.iter().chain(&actions.global_actions)
        .filter_map(|action| action.params["effect"]["id"].as_u64())
        .map(|id| json!({"id":id / 10,"sell_price":0,"plus_sell_price":0})).collect();
    evaluate_with_registry(state, actions, &json!({"amulets":amulets}))
}

fn candidate(kind: &str, id: Value, availability: Availability) -> ActionReport {
    ActionReport {
        stage: 16, interrupt: None, status: "ready".into(), issues: vec![], global_actions: vec![],
        actions: vec![ActionCandidate {
            id: "candidate-0".into(), kind: kind.into(), params: json!({"effect":{"id":id},"index":0}),
            cost: None, cost_resource: "coin".into(), availability, reasons: vec![],
        }],
    }
}

fn shop_state() -> Value {
    json!({"source":"packet","stage":9,"provider_state":{
        "state":{"current":9,"interrupt":0},"game":{"coin":"10"},"character":{"hp":50},
        "effect":{"effectList":[],"shopBuffList":[],"packCandidates":[],"maxEffectVolume":8,"packDisableSkip":false},
        "shop":{"goods":[],"upgradeGoods":[],"refreshPrice":1}
    }})
}

fn pack_shop_state(guidance: usize, goods: Value) -> Value {
    let mut state = shop_state();
    state["provider_state"]["game"]["coin"] = json!(103);
    state["provider_state"]["effect"]["effectList"] = json!((0..guidance)
        .map(|index| json!({"id":90,"uid":index + 1,"volume":1,"badgeId":600070})).collect::<Vec<_>>());
    state["provider_state"]["shop"]["goods"] = goods;
    state["provider_state"]["shop"]["refreshPrice"] = json!(200);
    state
}

fn bonfire_state() -> Value {
    json!({"source":"packet","stage":13,"provider_state":{
        "state":{"current":13,"interrupt":0},"game":{"coin":49},"character":{"hp":59,"maxHp":100},
        "effect":{"effectList":[],"maxEffectVolume":2},
        "nodeEvent":{"bonfireSelection":[1,2,3,4,5,6,7,8,9]}
    }})
}

#[test]
fn bonfire_scores_real_options_at_strict_hp_and_coin_thresholds() {
    for (hp, coin, heal, coins, maximum) in [
        (json!(59), json!(49), 20.0, 20.0, 5.0),
        (json!(60), json!(50), 0.0, 10.0, 5.0),
        (json!(61), json!(51), 0.0, 10.0, 25.0),
        (json!(0), json!(0), 20.0, 20.0, 5.0),
        (json!("59.999999999999999999"), json!("49.999999999999999999"), 20.0, 20.0, 5.0),
        (json!("6e1"), json!("5.0e1"), 0.0, 10.0, 5.0),
        (json!("1e100000"), json!("50.000000000000000001"), 0.0, 10.0, 25.0),
    ] {
        let mut state = bonfire_state();
        state["provider_state"]["character"]["hp"] = hp;
        state["provider_state"]["game"]["coin"] = coin;
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        for action in actions.actions.iter().chain(&actions.global_actions) {
            let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
            let expected = match action.params["id"].as_u64() {
                Some(1) => heal, Some(2) => maximum, Some(4) => coins, Some(5..=9) => 10.0, _ => 0.0,
            };
            assert_eq!(score.total, Some(expected), "{:?}, {state}", action.params);
            assert_eq!(score.rules.iter().any(|rule| rule.rule_id == "bonfire"),
                action.kind == "bonfire" && action.params["id"] != 3);
            if matches!(action.params["id"].as_u64(), Some(5..=9)) { assert!(expected <= coins); }
        }
    }
}

#[test]
fn bonfire_amulets_use_actual_capacity_including_seals_and_game_fallback() {
    for (owned, maximum, expected) in [
        (json!([]), 2, 10.0), (json!([]), 0, 5.0),
        (json!([{"volume":1}]), 2, 10.0), (json!([{"volume":2}]), 2, 5.0),
        (json!([{"volume":3}]), 2, 5.0),
        (json!([{"badgeId":0},{"badgeId":600050}]), 2, 5.0),
        (json!([{"badgeId":600160}]), 2, 5.0),
        (json!([{"badge":{"id":600160}}]), 3, 10.0),
        (json!([{"badgeId":600160,"volume":0}]), 1, 10.0),
    ] {
        for fallback in [false, true] {
            let mut state = bonfire_state();
            state["provider_state"]["effect"]["effectList"] = owned.clone();
            state["provider_state"]["game"]["maxEffectVolume"] = json!(maximum);
            if fallback { state["provider_state"]["effect"].as_object_mut().unwrap().remove("maxEffectVolume"); }
            else { state["provider_state"]["effect"]["maxEffectVolume"] = json!(maximum); }
            let actions = shanten_backend::action_provider::evaluate(&state);
            let report = evaluate(&state, &actions);
            for action in actions.actions.iter().filter(|a| matches!(a.params["id"].as_u64(), Some(5..=9))) {
                let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
                assert_eq!(score.total, Some(expected), "{owned}, max {maximum}, fallback {fallback}");
                assert_eq!(score.rules[0].reason.code, "bonfire_amulet");
            }
        }
    }
}

#[test]
fn bonfire_missing_or_invalid_resources_are_not_treated_as_empty() {
    for (path, value, choices, reason) in [
        ("/character/hp", Value::Null, vec![1], "missing_bonfire_hp"),
        ("/character/hp", json!(-1), vec![1], "missing_bonfire_hp"),
        ("/game/coin", Value::Null, vec![2,4], "missing_bonfire_coin"),
        ("/game/coin", json!("NaN"), vec![2,4], "missing_bonfire_coin"),
        ("/game/coin", json!(-1), vec![2,4], "missing_bonfire_coin"),
        ("/effect/effectList", Value::Null, vec![5,6,7,8,9], "missing_bonfire_capacity"),
        ("/effect/effectList", json!([{}]), vec![5], "missing_bonfire_capacity"),
        ("/effect/effectList", json!([{"badgeId":999999}]), vec![5], "missing_bonfire_capacity"),
        ("/effect/effectList", json!([{"volume":u64::MAX},{"volume":1}]), vec![5], "missing_bonfire_capacity"),
        ("/effect/maxEffectVolume", Value::Null, vec![5], "missing_bonfire_capacity"),
        ("/effect/maxEffectVolume", json!(-1), vec![5], "missing_bonfire_capacity"),
    ] {
        let mut state = bonfire_state();
        *state["provider_state"].pointer_mut(path).unwrap() = value;
        let mut actions = candidate("bonfire", Value::Null, Availability::Available);
        for choice in choices {
            actions.actions[0].params = json!({"id":choice});
            let report = evaluate(&state, &actions);
            assert_eq!(report.actions[0].total, Some(0.0));
            assert_eq!(report.actions[0].rules[0].score, None);
            assert_eq!(report.actions[0].rules[0].reason.code, reason);
        }
    }
}

#[test]
fn bonfire_bonuses_stack_with_costs_and_only_score_available_recognized_choices() {
    let state = bonfire_state();
    let mut actions = candidate("bonfire", Value::Null, Availability::Available);
    actions.actions[0].params = json!({"id":4});
    actions.actions[0].cost = Some("3".into());
    assert_eq!(evaluate(&state, &actions).actions[0].total, Some(17.0));
    for availability in [Availability::Unavailable, Availability::Unknown] {
        actions.actions[0].availability = availability;
        assert!(evaluate(&state, &actions).actions.is_empty());
    }
    actions.actions[0].availability = Availability::Available;
    actions.actions[0].cost = None;
    for id in [json!(0), json!(3), json!(10), json!("4"), Value::Null] {
        actions.actions[0].params = json!({"id":id});
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
    }
    actions.actions[0].kind = "next_node".into();
    actions.actions[0].params = json!({"id":4});
    assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
}

#[test]
fn shop_refresh_uses_the_strict_remaining_balance_threshold_and_keeps_costs() {
    let mut state = shop_state();
    for (coin, cost, bonus) in [
        (json!(7), 2, 0.0), (json!(8), 2, 20.0), (json!(5), 0, 0.0), (json!(6), 0, 20.0),
        (json!("7.00000000000000000001"), 2, 20.0), (json!("6.99999999999999999999"), 2, 0.0),
        (json!("7.0e0"), 2, 0.0), (json!("1e100000"), 2, 20.0),
    ] {
        state["provider_state"]["game"]["coin"] = coin.clone();
        state["provider_state"]["shop"]["refreshPrice"] = json!(cost);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        for (kind, expected) in [("refresh_shop", if bonus > 0.0 { bonus } else { -f64::from(cost) }), ("next_node", -bonus)] {
            let action = actions.actions.iter().find(|action| action.kind == kind).unwrap();
            let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
            assert_eq!(score.total, Some(expected), "{kind}, coin {coin}, cost {cost}");
            assert_eq!(score.rules.iter().any(|rule| rule.rule_id == "shop_refresh"), bonus > 0.0);
            if bonus > 0.0 {
                assert_eq!(score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").unwrap().score,
                    Some(if kind == "refresh_shop" { bonus + f64::from(cost) } else { -bonus }));
            }
        }
    }
    state["provider_state"]["effect"]["effectList"] = json!([{"id":3630,"uid":1,"volume":1}]);
    state["provider_state"]["shop"]["refreshPrice"] = json!(3);
    for (coin, bonus) in [(5,0.0), (6,20.0)] {
        state["provider_state"]["game"]["coin"] = json!(coin);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        let refresh = actions.actions.iter().find(|action| action.kind == "refresh_shop").unwrap();
        assert_eq!(refresh.cost_resource, "hp");
        let score = report.actions.iter().find(|score| score.action_id == refresh.id).unwrap();
        let hp_cost = refresh.cost.as_deref().unwrap().parse::<f64>().unwrap();
        assert_eq!(score.total, Some(bonus - 2.0 * hp_cost));
        assert_eq!(score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").and_then(|rule| rule.score),
            if bonus > 0.0 { Some(bonus) } else { None });
        let next = actions.actions.iter().find(|action| action.kind == "next_node").unwrap();
        assert_eq!(report.actions.iter().find(|score| score.action_id == next.id).unwrap().total, Some(-bonus));
    }
}

#[test]
fn shop_refresh_bonus_waits_for_affordable_unsold_amulet_packs() {
    let mut state = shop_state();
    for id in [101,102,103,111,112,113] {
        for (price, sold, bonus) in [(5,json!(false),0.0), (10,json!(0),0.0), (0,json!(false),0.0),
            (11,json!(false),20.0), (5,json!(true),20.0), (5,json!(1),20.0)] {
            state["provider_state"]["shop"]["goods"] = json!([{"id":1,"goodsId":id,"price":price,"sold":sold}]);
            let actions = shanten_backend::action_provider::evaluate(&state);
            let report = evaluate(&state, &actions);
            for (kind, expected) in [("refresh_shop", if bonus > 0.0 { bonus } else { -1.0 }), ("next_node", if bonus == 0.0 { -5.0 } else { -20.0 })] {
                let action = actions.actions.iter().find(|action| action.kind == kind).unwrap();
                let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
                assert_eq!(score.total, Some(expected), "{kind}, pack {id}, price {price}, sold {sold}");
            }
        }
    }
    for (goods, expected) in [
        (json!([{"id":1,"goodsId":201,"price":1,"sold":false}]), Some(21.0)),
        (json!([{"id":1,"goodsId":101,"price":1}]), None),
        (json!([{"id":1,"goodsId":101,"price":"invalid","sold":false}]), None),
        (Value::Null, None), (json!({}), None),
    ] {
        state["provider_state"]["shop"]["goods"] = goods;
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        let refresh = actions.actions.iter().find(|action| action.kind == "refresh_shop").unwrap();
        let score = report.actions.iter().find(|score| score.action_id == refresh.id).unwrap();
        let rule = score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").unwrap();
        assert_eq!(rule.score, expected);
        assert_eq!(score.total, Some(expected.unwrap_or(0.0) - 1.0));
        assert_eq!(rule.reason.code, if expected.is_some() { "shop_refresh" } else { "missing_shop_packs" });
    }
}

#[test]
fn leaving_affordable_packs_deducts_five_once_without_requiring_a_refresh_budget() {
    let mut state = shop_state();
    state["provider_state"]["game"]["coin"] = json!(3);
    state["provider_state"]["shop"]["goods"] = json!([
        {"id":1,"goodsId":101,"price":1,"sold":false},
        {"id":2,"goodsId":102,"price":3,"sold":false}
    ]);
    for cost in [json!(0), json!(1), json!(3), json!(4), Value::Null] {
        state["provider_state"]["shop"]["refreshPrice"] = cost;
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        let next = actions.actions.iter().find(|action| action.kind == "next_node").unwrap();
        let score = report.actions.iter().find(|score| score.action_id == next.id).unwrap();
        assert_eq!(score.total, Some(-5.0));
        assert_eq!(score.rules.len(), 1);
        assert_eq!(score.rules[0].score, Some(-5.0));
        assert_eq!(score.rules[0].reason.code, "leave_shop_affordable_pack");
    }
    state["stage"] = json!(3);
    state["provider_state"]["state"]["current"] = json!(3);
    let actions = shanten_backend::action_provider::evaluate(&state);
    let report = evaluate(&state, &actions);
    assert!(report.actions.iter().all(|score| score.total == Some(0.0) && score.rules.is_empty()));
}

#[test]
fn shop_refresh_requires_a_legal_refresh_in_a_normal_shop_and_known_balance() {
    let mut state = shop_state();
    let actions = shanten_backend::action_provider::evaluate(&state);
    for availability in [Availability::Unknown, Availability::Unavailable] {
        let mut actions = actions.clone();
        actions.actions.iter_mut().find(|action| action.kind == "refresh_shop").unwrap().availability = availability;
        assert!(evaluate(&state, &actions).actions.iter().all(|score| score.rules.iter().all(|rule| rule.rule_id != "shop_refresh")));
    }
    for stage in [3,11,16] {
        state["stage"] = json!(stage);
        assert!(evaluate(&state, &actions).actions.iter().all(|score| score.rules.iter().all(|rule| rule.rule_id != "shop_refresh")));
    }
    state["stage"] = json!(9);
    for (coin, cost) in [(Value::Null, Some("1")), (json!("NaN"), Some("1")), (json!(-10), Some("1")),
        (json!(10), None), (json!(10), Some("bad")), (json!(10), Some("-1")),
        (json!("1e100000"), Some("340282366920938463463374607431768211455"))] {
        state["provider_state"]["game"]["coin"] = coin;
        let mut actions = actions.clone();
        actions.actions.iter_mut().find(|action| action.kind == "refresh_shop").unwrap().cost = cost.map(String::from);
        let report = evaluate(&state, &actions);
        for kind in ["refresh_shop", "next_node"] {
            let action = actions.actions.iter().find(|action| action.kind == kind).unwrap();
            let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
            let rule = score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").unwrap();
            assert_eq!(rule.score, None);
            assert_eq!(rule.reason.code, "missing_shop_refresh_balance");
        }
    }
    state = shop_state();
    state["provider_state"]["state"]["interrupt"] = json!(1000);
    let actions = shanten_backend::action_provider::evaluate(&state);
    assert!(evaluate(&state, &actions).actions.iter().all(|score| score.rules.iter().all(|rule| rule.rule_id != "shop_refresh")));
}

#[test]
fn expensive_refresh_outscores_selling_a_retained_amulet_in_the_full_inventory_screenshot() {
    let mut state = shop_state();
    let mut owned = vec![json!({"id":3561,"uid":1,"volume":1,"badgeId":0})];
    owned.extend((2..=8).map(|uid| json!({"id":90,"uid":uid,"volume":1,"badgeId":0})));
    state["provider_state"]["effect"]["effectList"] = json!(owned);
    state["provider_state"]["shop"]["refreshPrice"] = json!(27);
    state["provider_state"]["shop"]["goods"] = json!([
        {"id":1,"goodsId":101,"price":5,"sold":true},
        {"id":2,"goodsId":102,"price":10,"sold":true},
        {"id":3,"goodsId":103,"price":15,"sold":true}
    ]);
    let registry = json!({"amulets":[{"id":356,"sell_price":9,"plus_sell_price":9},{"id":9,"sell_price":0}]});
    for (coin, expected) in [(139,20.0),(32,-27.0),(33,20.0)] {
        state["provider_state"]["game"]["coin"] = json!(coin);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        let refresh = actions.actions.iter().find(|action| action.kind == "refresh_shop").unwrap();
        let refresh_score = report.actions.iter().find(|score| score.action_id == refresh.id).unwrap();
        assert_eq!(refresh_score.total, Some(expected));
        assert_eq!(refresh_score.rules.iter().find(|rule| rule.rule_id == "coin_cost").unwrap().score, Some(-27.0));
        assert_eq!(refresh_score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").and_then(|rule| rule.score),
            if coin > 32 { Some(47.0) } else { None });
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 1).unwrap();
        let sale_score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(sale_score.total, Some(-6.0));
        assert_eq!(sale_score.rules.iter().find(|rule| rule.rule_id == "retain_amulet_spare_capacity").unwrap().score,
            Some(-10.0));
        assert_eq!(refresh_score.total.unwrap() > sale_score.total.unwrap(), coin > 32);
        let next = actions.actions.iter().find(|action| action.kind == "next_node").unwrap();
        assert_eq!(report.actions.iter().find(|score| score.action_id == next.id).unwrap().total,
            Some(if coin > 32 { -20.0 } else { 0.0 }));
    }
}

#[test]
fn full_inventory_with_six_guidance_seals_prefers_a_pack_over_selling_retained_cards() {
    let mut state = shop_state();
    let mut owned = vec![json!({"id":3561,"uid":1,"volume":1,"badgeId":0}),
        json!({"id":2240,"uid":2,"volume":1,"badgeId":0})];
    owned.extend([10,20,30,90,100,120].into_iter().enumerate()
        .map(|(index, id)| json!({"id":id,"uid":index + 3,"volume":1,"badgeId":600070})));
    state["provider_state"]["effect"]["effectList"] = json!(owned);
    state["provider_state"]["game"]["coin"] = json!(91);
    state["provider_state"]["shop"]["refreshPrice"] = json!(93);
    state["provider_state"]["shop"]["goods"] = json!([
        {"id":1,"goodsId":101,"price":5,"sold":false},
        {"id":2,"goodsId":102,"price":10,"sold":false},
        {"id":3,"goodsId":103,"price":15,"sold":false}
    ]);
    let registry = json!({"amulets":[{"id":356,"sell_price":9,"plus_sell_price":9},{"id":224,"sell_price":9},
        {"id":1,"sell_price":3},{"id":2,"sell_price":3},{"id":3,"sell_price":3},
        {"id":9,"sell_price":9},{"id":10,"sell_price":9},{"id":12,"sell_price":9}]});
    let actions = shanten_backend::action_provider::evaluate(&state);
    let report = evaluate_with_registry(&state, &actions, &registry);
    assert_eq!(actions.actions.iter().find(|action| action.kind == "refresh_shop").unwrap().availability, Availability::Unavailable);
    let pack = actions.actions.iter().find(|action| action.kind == "buy_pack" && action.params["goods_id"] == 101).unwrap();
    assert_eq!(pack.availability, Availability::Available);
    let pack_score = report.actions.iter().find(|score| score.action_id == pack.id).unwrap();
    let value = pack_score.rules.iter().find(|rule| rule.rule_id == "pack_value").unwrap().score.unwrap();
    assert!((value - 8.99).abs() < 0.01);
    assert!((pack_score.total.unwrap() - 9.0).abs() < 1e-12);
    for sale in actions.actions.iter().filter(|action| action.kind == "sell_effect") {
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        if sale.params["uid"] == 1 {
            assert_eq!(score.total, Some(-6.0));
            assert!(pack_score.total.unwrap() > score.total.unwrap());
            assert_eq!(score.rules.iter().find(|rule| rule.rule_id == "retain_amulet_spare_capacity").unwrap().score, Some(-10.0));
        } else if sale.params["effect"]["badgeId"] == 600070 {
            let expected = if matches!(sale.params["effect"]["id"].as_u64(), Some(10 | 20 | 30)) { 2.0 } else { 8.0 };
            assert_eq!(score.total, Some(expected)); // Sale price + 5 minus the sixth seal's 6 points.
        }
    }
}

#[test]
fn affordable_pack_sets_are_bought_in_rarity_order_using_the_best_pack_within_a_tier() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    for (goods, order) in [
        (json!([{"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}]),vec![1,2]),
        (json!([{"id":1,"goodsId":101,"price":5,"sold":false},{"id":2,"goodsId":102,"price":10,"sold":false},
            {"id":3,"goodsId":103,"price":15,"sold":false}]),vec![1,2,3]),
        (json!([{"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":112,"price":1,"sold":false},
            {"id":3,"goodsId":113,"price":15,"sold":false}]),vec![2,1,3]),
    ] {
        let mut state = pack_shop_state(5, goods);
        for (step, id) in order.iter().enumerate() {
            let actions = shanten_backend::action_provider::evaluate(&state);
            let report = evaluate_with_registry(&state, &actions, &registry);
            let pack = actions.actions.iter().find(|action| action.kind == "buy_pack" && action.params["id"] == *id).unwrap();
            let score = report.actions.iter().find(|score| score.action_id == pack.id).unwrap();
            assert!(report.actions.iter().filter(|other| other.action_id != pack.id)
                .all(|other| other.total.is_some_and(|total| total < score.total.unwrap())), "order {order:?}, step {step}");
            if order[0] == 2 && step == 0 {
                assert!(score.rules.iter().all(|rule| rule.rule_id != "pack_purchase_order"));
            }
            let good = state["provider_state"]["shop"]["goods"].as_array_mut().unwrap().iter_mut()
                .find(|good| good["id"] == *id).unwrap();
            good["sold"] = json!(true);
            let cost = good["price"].as_u64().unwrap();
            let coin = state["provider_state"]["game"]["coin"].as_u64().unwrap();
            state["provider_state"]["game"]["coin"] = json!(coin - cost);
        }
    }
}

#[test]
fn available_packs_keep_guidance_until_opening_even_when_the_whole_set_cannot_be_bought() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    for (blue_price, gold_price, gold_sold, ordered, has_available_pack) in [
        (10,json!(15),json!(false),false,true),
        (10,json!(15),json!(true),true,true),
        (10,json!(15),Value::Null,false,true),
        (10,json!("invalid"),json!(false),false,true),
        (16,json!(17),json!(false),false,false),
    ] {
        let mut state = pack_shop_state(5, json!([
            {"id":1,"goodsId":102,"price":blue_price,"sold":false},
            {"id":2,"goodsId":103,"price":gold_price,"sold":gold_sold}
        ]));
        state["provider_state"]["game"]["coin"] = json!(15);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        assert_eq!(report.actions.iter().any(|score| score.rules.iter().any(|rule| rule.rule_id == "pack_purchase_order")), ordered);
        let best_pack = actions.actions.iter().filter(|action| action.kind == "buy_pack" && action.availability == Availability::Available)
            .filter_map(|action| report.actions.iter().find(|score| score.action_id == action.id)?.total).reduce(f64::max);
        assert_eq!(best_pack.is_some(), has_available_pack);
        for sale in actions.actions.iter().filter(|action| action.kind == "sell_effect") {
            let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
            if let Some(best) = best_pack {
                assert!(score.total.unwrap() < best);
                if !ordered { assert!((score.total.unwrap() - (best - 1.0)).abs() < 1e-12); }
            } else {
                assert_eq!(score.total, Some(4.0));
                assert!(score.rules.iter().all(|rule| rule.rule_id != "preserve_guidance_for_packs"));
            }
        }
    }
}

#[test]
fn pack_order_uses_the_combined_coin_and_hp_budgets_with_strict_hp_survival() {
    for (pay_hp, coin, hp, ordered) in [
        (false,json!(25),Value::Null,true),(false,json!(24),Value::Null,false),
        (false,Value::Null,Value::Null,false),
        (true,Value::Null,json!(26),true),(true,Value::Null,json!(25),false),
        (true,Value::Null,Value::Null,false),
    ] {
        let mut state = pack_shop_state(0, json!([
            {"id":1,"goodsId":102,"price":10,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}
        ]));
        state["provider_state"]["game"]["coin"] = coin;
        state["provider_state"]["character"]["hp"] = hp;
        if pay_hp { state["provider_state"]["effect"]["effectList"] = json!([{"id":3631,"uid":1,"volume":1,"badgeId":0}]); }
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions.iter().any(|score| score.rules.iter().any(|rule| rule.rule_id == "pack_purchase_order")),
            ordered, "{state}");
    }
}

#[test]
fn pack_priority_does_not_invent_unknown_or_out_of_range_scores() {
    for (price, owned) in [
        (json!("9007199254740991"),json!([])),
        (json!("340282366920938463463374607431768211455"),json!([])),
        (json!(10),json!([{"id":90,"uid":1,"volume":1,"badgeId":"invalid"}])),
    ] {
        let mut state = pack_shop_state(0, json!([
            {"id":1,"goodsId":102,"price":price,"sold":false},{"id":2,"goodsId":103,"price":1,"sold":false}
        ]));
        state["provider_state"]["game"]["coin"] = json!("1e100000");
        state["provider_state"]["effect"]["effectList"] = owned;
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        for rule in report.actions.iter().flat_map(|score| &score.rules).filter(|rule|
            matches!(rule.rule_id.as_str(), "pack_purchase_order" | "preserve_guidance_for_packs")) {
            assert_eq!(rule.score, None);
            assert_eq!(rule.reason.code, "invalid_pack_priority");
        }
    }
}

#[test]
fn full_guidance_inventory_buys_a_pack_before_sales_but_allows_sales_during_selection() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9},{"id":339,"sell_price":6}]});
    let mut state = pack_shop_state(8, json!([
        {"id":1,"goodsId":101,"price":5,"sold":false},{"id":2,"goodsId":103,"price":15,"sold":false}
    ]));
    let actions = shanten_backend::action_provider::evaluate(&state);
    let report = evaluate_with_registry(&state, &actions, &registry);
    let pack = actions.actions.iter().find(|action| action.kind == "buy_pack" && action.params["goods_id"] == 101).unwrap();
    let pack_score = report.actions.iter().find(|score| score.action_id == pack.id).unwrap().total.unwrap();
    for sale in actions.actions.iter().filter(|action| action.kind == "sell_effect") {
        assert!(pack_score > report.actions.iter().find(|score| score.action_id == sale.id).unwrap().total.unwrap());
    }
    state["provider_state"]["effect"]["packCandidates"] = json!([{"id":3390,"badgeId":0}]);
    for (stage, interrupt) in [(9,1000),(16,0)] {
        state["stage"] = json!(stage);
        state["provider_state"]["state"]["interrupt"] = json!(interrupt);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        for sale in actions.actions.iter().filter(|action| action.kind == "sell_effect") {
            assert_eq!(sale.availability, Availability::Available);
            assert_eq!(report.actions.iter().find(|score| score.action_id == sale.id).unwrap().total, Some(18.5));
        }
        assert!(report.actions.iter().all(|score| score.rules.iter().all(|rule|
            rule.rule_id != "pack_purchase_order" && rule.rule_id != "preserve_guidance_for_packs")));
    }
}

#[test]
fn shop_refresh_priority_bonus_preserves_the_exact_score_limit() {
    const MAX_EXACT: u64 = 9_007_199_254_740_991;
    let mut state = shop_state();
    state["provider_state"]["game"]["coin"] = json!("1e100000");
    for cost in [MAX_EXACT - 20,MAX_EXACT - 19,MAX_EXACT] {
        state["provider_state"]["shop"]["refreshPrice"] = json!(cost.to_string());
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate(&state, &actions);
        let refresh = actions.actions.iter().find(|action| action.kind == "refresh_shop").unwrap();
        let score = report.actions.iter().find(|score| score.action_id == refresh.id).unwrap();
        let rule = score.rules.iter().find(|rule| rule.rule_id == "shop_refresh").unwrap();
        if cost == MAX_EXACT - 20 {
            assert_eq!(rule.score, Some(MAX_EXACT as f64));
            assert_eq!(rule.reason.code, "shop_refresh");
            assert_eq!(score.total, Some(20.0));
        } else {
            assert_eq!(rule.score, None);
            assert_eq!(rule.reason.code, "invalid_shop_refresh_score");
            assert_eq!(score.total, Some(-(cost as f64)));
        }
        let next = actions.actions.iter().find(|action| action.kind == "next_node").unwrap();
        assert_eq!(report.actions.iter().find(|score| score.action_id == next.id).unwrap().total, Some(-20.0));
    }
}

#[test]
fn ryan_scores_normal_and_plus_candidates_only_without_owned_plus() {
    for id in [2240, 2241] {
        for (owned, total, reason) in [
            (json!([]), 50, "ryan_missing"),
            (json!([{"id":3020}]), 50, "ryan_missing"),
            (json!([{"id":2240}]), 50, "ryan_not_plus"),
            (json!([{"id":2240},{"id":2240}]), 50, "ryan_not_plus"),
            (json!([{"id":2241}]), 0, "ryan_already_plus"),
            (json!([{"id":2240},{"id":2241}]), 0, "ryan_already_plus"),
        ] {
            let state = json!({"provider_state":{"effect":{"effectList":owned}}});
            let report = evaluate(&state, &candidate("select_candidate", json!(id), Availability::Available));
            let score = &report.actions[0];
            assert_eq!(score.action_id, "candidate-0");
            assert_eq!(score.total, Some(total as f64), "candidate {id}, owned {owned}");
            assert_eq!(score.rules.len(), 1);
            assert_eq!(score.rules[0].rule_id, "ryan_without_plus");
            assert_eq!(score.rules[0].score, Some(total as f64));
            assert_eq!(score.rules[0].reason.code, reason);
        }
    }
}

#[test]
fn ryan_sale_reverses_the_selection_bonus_based_on_the_remaining_inventory() {
    for id in [2240,2241] {
        for (remaining, penalty) in [
            (json!([]), -50.0), (json!([{"id":2240}]), -50.0),
            (json!([{"id":2240},{"id":2240}]), -50.0),
            (json!([{"id":2241}]), 0.0),
            (json!([{"id":2240},{"id":2241}]), 0.0),
            (json!([{"id":2241},{"id":2241}]), 0.0),
        ] {
            let after = json!({"provider_state":{"effect":{"effectList":remaining}}});
            let mut before = after.clone();
            before["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(json!({"id":id}));
            let sale = evaluate(&before, &candidate("sell_effect", json!(id), Availability::Available));
            let selection = evaluate(&after, &candidate("select_candidate", json!(id), Availability::Available));
            assert_eq!(sale.actions[0].total, Some(5.0 + penalty), "selling {id}, remaining {remaining}");
            let rule = &sale.actions[0].rules[0];
            assert_eq!(rule.rule_id, "ryan_without_plus");
            assert_eq!(rule.score, Some(penalty));
            assert_eq!(rule.score, selection.actions[0].rules[0].score.map(|score| -score));
            assert_eq!(rule.reason.code, if penalty == 0.0 { "sell_ryan_plus_remains" } else { "sell_ryan" });
        }
    }
}

#[test]
fn missing_or_malformed_inventory_is_not_an_empty_inventory() {
    for state in [
        json!({"effect_list":[]}),
        json!({"provider_state":{"effect":{"effectList":null}}}),
        json!({"provider_state":{"effect":{"effectList":{}}}}),
        json!({"provider_state":{"effect":{"effectList":[{}, {"id":2240}]}}}),
        json!({"provider_state":{"effect":{"effectList":[{"id":0}]}}}),
        json!({"provider_state":{"effect":{"effectList":[{"id":"2241"}]}}}),
        json!({"provider_state":{"effect":{"effectList":[{"id":-1}]}}}),
    ] {
        for kind in ["select_candidate", "sell_effect"] {
            for id in [2240,2241] {
                let report = evaluate(&state, &candidate(kind, json!(id), Availability::Available));
                assert_eq!(report.actions[0].total, Some(if kind == "sell_effect" { 5.0 } else { 0.0 }), "{state}");
                assert_eq!(report.actions[0].rules[0].score, None);
                assert_eq!(report.actions[0].rules[0].reason.code, "missing_effects");
            }
        }
    }
}

#[test]
fn unrelated_or_illegal_actions_do_not_receive_the_bonus() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    for (kind, id) in [("buy_effect", json!(2240)), ("sell_effect", json!(3020)),
        ("select_candidate", json!(3020)), ("select_candidate", json!(224)), ("select_candidate", Value::Null)] {
        let report = evaluate(&state, &candidate(kind, id, Availability::Available));
        assert_eq!(report.actions[0].total, Some(if kind == "sell_effect" { 5.0 } else { 0.0 }));
        assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "ryan_without_plus"));
    }
    for availability in [Availability::Unavailable, Availability::Unknown] {
        for kind in ["select_candidate", "sell_effect"] {
            assert!(evaluate(&state, &candidate(kind, json!(2240), availability.clone())).actions.is_empty());
        }
    }
}

#[test]
fn every_available_amulet_sale_receives_five_points_once() {
    for id in [2240,3020,3021,3550] {
        let mut actions = candidate("sell_effect", json!(id), Availability::Available);
        for badge in [0,600050,600170] {
            actions.actions[0].params["effect"]["badgeId"] = json!(badge);
            let report = evaluate(&json!({}), &actions);
            assert_eq!(report.actions[0].total, Some(5.0));
            let rules: Vec<_> = report.actions[0].rules.iter().filter(|rule| rule.rule_id == "sell_bonus").collect();
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].score, Some(5.0));
            assert_eq!(rules[0].reason.code, "sell_bonus");
        }
        for availability in [Availability::Unknown, Availability::Unavailable] {
            actions.actions[0].availability = availability;
            assert!(evaluate(&json!({}), &actions).actions.is_empty());
        }
    }
    for kind in ["select_candidate", "buy_effect", "buy_pack", "forge_effect", "shop_upgrade", "next_node", "skip"] {
        let actions = candidate(kind, json!(3020), Availability::Available);
        assert!(evaluate(&json!({}), &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "sell_bonus"));
    }
}

#[test]
fn preferred_amulets_and_their_plus_variants_stack_both_retention_penalties_without_a_better_choice() {
    for id in [620,621,720,721,1460,1461,3560,3561] {
        let actions = candidate("sell_effect", json!(id), Availability::Available);
        let report = evaluate(&json!({}), &actions);
        assert_eq!(report.actions[0].total, Some(-15.0), "effect {id}");
        assert_eq!(report.actions[0].rules.iter().map(|rule| (rule.rule_id.as_str(), rule.score)).collect::<Vec<_>>(),
            vec![("sell_bonus", Some(5.0)), ("retain_amulet", Some(-10.0)), ("retain_amulet_spare_capacity", Some(-10.0))]);
        assert_eq!(report.actions[0].rules[1].reason.code, "retain_amulet");
        for kind in ["select_candidate", "buy_effect", "forge_effect", "skip"] {
            let actions = candidate(kind, json!(id), Availability::Available);
            assert!(evaluate(&json!({}), &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "retain_amulet"));
        }
        for availability in [Availability::Unknown, Availability::Unavailable] {
            let actions = candidate("sell_effect", json!(id), availability);
            assert!(evaluate(&json!({}), &actions).actions.is_empty());
        }
    }
    for id in [json!(3020), json!(62), json!(629), json!(3562), json!("620"), json!(-620), Value::Null] {
        let actions = candidate("sell_effect", id, Availability::Available);
        let report = evaluate(&json!({}), &actions);
        assert_eq!(report.actions[0].total, Some(5.0));
        assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "retain_amulet"));
    }
}

#[test]
fn preferred_amulet_sale_penalty_accumulates_with_target_and_seal_rules() {
    for (badge, expected) in [(600070,-537.0), (600110,-522.0)] {
        let mut actions = candidate("sell_effect", json!(620), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(badge);
        let state = json!({"provider_state":{"effect":{"effectList":[actions.actions[0].params["effect"]]}}});
        let book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
            "targets":[{"kind":"amulet","id":62,"value":2},{"kind":"badge","id":badge,"value":3}]}]});
        let registry = json!({"amulets":[{"id":62,"sell_price":3}]});
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(expected)); // 5 sale + 3 price - 20 retention - 495 target - seal penalty
        assert_eq!(report.actions[0].rules.len(), 6);
        let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "retain_amulet").unwrap();
        assert_eq!(rule.score, Some(-10.0));
    }
}

#[test]
fn stage_and_global_actions_start_at_zero_without_applicable_rules() {
    let mut actions = candidate("skip", Value::Null, Availability::Available);
    let mut global = actions.actions[0].clone();
    global.id = "remake".into();
    global.kind = "remake".into();
    actions.global_actions.push(global);
    let report = evaluate(&json!({}), &actions);
    assert_eq!(report.actions.len(), 2);
    assert!(report.actions.iter().all(|score| score.total == Some(0.0) && score.rules.is_empty()));
    let round_trip: decision::DecisionReport = serde_json::from_value(serde_json::to_value(report).unwrap()).unwrap();
    assert!(round_trip.actions.iter().all(|score| score.total == Some(0.0)));
}

#[test]
fn every_coin_spending_action_deducts_its_actual_cost() {
    for kind in ["buy_pack", "buy_effect", "buy_rune", "shop_upgrade", "advanced_upgrade",
        "refresh_shop", "forge_effect", "trade", "transport", "gamble", "future_paid_action"] {
        let mut actions = candidate(kind, json!(3020), Availability::Available);
        for cost in [1i64, 10, u32::MAX as i64, 9_007_199_254_740_991] {
            actions.actions[0].cost = Some(cost.to_string());
            let report = evaluate(&json!({}), &actions);
            let score = &report.actions[0];
            let upgrade = matches!(kind, "shop_upgrade" | "advanced_upgrade");
            let expected = if upgrade && cost == 9_007_199_254_740_991 { None }
                else { Some(-(cost as f64) - if upgrade { 10.0 } else { 0.0 }) };
            assert_eq!(score.total, expected, "{kind}: {cost}");
            let coin = score.rules.iter().find(|rule| rule.rule_id == "coin_cost").unwrap();
            assert_eq!(coin.score, Some(-cost as f64));
            assert_eq!(coin.reason.code, "coin_cost");
        }
    }
}

#[test]
fn coin_rule_skips_free_non_coin_and_illegal_actions() {
    let mut actions = candidate("refresh_shop", Value::Null, Availability::Available);
    for (resource, cost) in [("coin", None), ("coin", Some("0")), ("hp", Some("10"))] {
        actions.actions[0].cost_resource = resource.into();
        actions.actions[0].cost = cost.map(String::from);
        let report = evaluate(&json!({}), &actions);
        assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "coin_cost"));
        assert_eq!(report.actions[0].total, Some(if resource == "hp" { -20.0 } else { 0.0 }));
    }
    actions.actions[0].cost_resource = "coin".into();
    actions.actions[0].cost = Some("10".into());
    for availability in [Availability::Unknown, Availability::Unavailable] {
        actions.actions[0].availability = availability;
        assert!(evaluate(&json!({}), &actions).actions.is_empty());
    }
}

#[test]
fn coin_penalty_accumulates_with_ryan_and_keeps_invalid_costs_unknown() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].cost = Some("10".into());
    let report = evaluate(&state, &actions);
    assert_eq!(report.actions[0].total, Some(40.0));
    assert_eq!(report.actions[0].rules.iter().map(|rule| rule.score).collect::<Vec<_>>(), vec![Some(50.0), Some(-10.0)]);
    for cost in ["-1", "not-a-price", "1.5", "9007199254740992", "340282366920938463463374607431768211455"] {
        actions.actions[0].cost = Some(cost.into());
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(50.0), "{cost}");
        assert_eq!(report.actions[0].rules[1].score, None);
        assert_eq!(report.actions[0].rules[1].reason.code, "invalid_coin_cost");
    }
}

#[test]
fn hp_payments_deduct_twice_the_already_converted_cost() {
    for kind in ["buy_pack", "buy_effect", "buy_rune", "shop_upgrade", "advanced_upgrade", "refresh_shop", "future_paid_action"] {
        let mut actions = candidate(kind, json!(3020), Availability::Available);
        actions.actions[0].cost_resource = "hp".into();
        actions.actions[0].cost = Some("10".into());
        for owned in [json!([]), json!([{"id":3630}]), json!([{"id":3631}])] {
            let state = json!({"provider_state":{"effect":{"effectList":owned}}});
            let report = evaluate(&state, &actions);
            let upgrade = matches!(kind, "shop_upgrade" | "advanced_upgrade");
            assert_eq!(report.actions[0].total, Some(if upgrade { -30.0 } else { -20.0 }), "{kind}: {owned}");
            assert_eq!(report.actions[0].rules.len(), if upgrade { 2 } else { 1 });
            assert_eq!(report.actions[0].rules[0].rule_id, "hp_loss");
            assert_eq!(report.actions[0].rules[0].reason.code, "hp_loss");
        }
    }
}

#[test]
fn all_shop_upgrade_items_receive_a_fixed_penalty_only_when_available() {
    for kind in ["shop_upgrade", "advanced_upgrade"] {
        let mut actions = candidate(kind, Value::Null, Availability::Available);
        for id in [8001,8002,8003,8004,9001] {
            actions.actions[0].params = json!({"upgrade_id":id});
            let report = evaluate(&json!({}), &actions);
            assert_eq!(report.actions[0].total, Some(-10.0), "{kind}: {id}");
            assert_eq!(report.actions[0].rules.len(), 1);
            assert_eq!(report.actions[0].rules[0].rule_id, "upgrade_penalty");
            assert_eq!(report.actions[0].rules[0].score, Some(-10.0));
            assert_eq!(report.actions[0].rules[0].reason.code, "upgrade_penalty");
        }
        for availability in [Availability::Unknown, Availability::Unavailable] {
            actions.actions[0].availability = availability;
            assert!(evaluate(&json!({}), &actions).actions.is_empty());
        }
    }
    for kind in ["buy_pack", "buy_effect", "refresh_shop", "forge_effect", "next_node"] {
        let actions = candidate(kind, Value::Null, Availability::Available);
        assert!(evaluate(&json!({}), &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "upgrade_penalty"));
    }
}

#[test]
fn acquisition_damage_is_charged_for_normal_plus_and_trade_rewards() {
    for (id, damage) in [(3550, 6), (3551, 12)] {
        for kind in ["select_candidate", "buy_effect", "trade"] {
            let mut actions = candidate(kind, json!(id), Availability::Available);
            let state = json!({"provider_state":{"effect":{"effectList":[]},
                "nodeEvent":{"tradeReward":{"type":1,"effectId":id}}}});
            if kind == "trade" {
                // The reward comes from the snapshot, not an invented action effect.
                actions.actions[0].params = json!({"uids":[],"hp_cost":"7"});
                actions.actions[0].cost = Some("10".into());
            }
            let report = evaluate(&state, &actions);
            let expected = if kind == "trade" { -10 - 2 * (7 + damage) } else { -2 * damage };
            assert_eq!(report.actions[0].total, Some(expected as f64), "{kind}: {id}");
            let hp = report.actions[0].rules.iter().find(|rule| rule.rule_id == "hp_loss").unwrap();
            assert_eq!(hp.score, Some((-2 * (damage + if kind == "trade" { 7 } else { 0 })) as f64));
        }
        let mut actions = candidate("buy_effect", json!(id), Availability::Available);
        actions.actions[0].cost_resource = "hp".into();
        actions.actions[0].cost = Some("10".into());
        assert_eq!(evaluate(&json!({}), &actions).actions[0].total, Some((-2 * (10 + damage)) as f64));
    }
    for kind in ["sell_effect", "forge_effect", "skip"] {
        assert!(evaluate(&json!({}), &candidate(kind, json!(3550), Availability::Available)).actions[0].rules.iter()
            .all(|rule| rule.rule_id != "hp_loss"));
    }
    let state = json!({"provider_state":{"nodeEvent":{"tradeReward":{"type":2,"effectId":3550}}}});
    assert!(evaluate(&state, &candidate("trade", json!(3550), Availability::Available)).actions[0].rules.is_empty());
}

#[test]
fn hp_loss_accumulates_with_coin_cost_and_ryan_bonus() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].cost = Some("10".into());
    for cost in [json!(7), json!("7")] {
        actions.actions[0].params["hp_cost"] = cost;
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(26.0));
        assert_eq!(report.actions[0].rules.iter().map(|rule| (rule.rule_id.as_str(), rule.score)).collect::<Vec<_>>(),
            vec![("ryan_without_plus", Some(50.0)), ("coin_cost", Some(-10.0)), ("hp_loss", Some(-14.0))]);
    }
}

#[test]
fn hp_loss_preserves_free_unknown_and_score_overflow_states() {
    let mut actions = candidate("refresh_shop", Value::Null, Availability::Available);
    actions.actions[0].cost_resource = "hp".into();
    actions.actions[0].cost = Some("0".into());
    actions.actions[0].params["hp_cost"] = json!("0");
    assert!(evaluate(&json!({}), &actions).actions[0].rules.is_empty());
    for cost in [None, Some("-1"), Some("bad"), Some("1.5"), Some("4503599627370496"), Some("9223372036854775807"), Some("18446744073709551615")] {
        actions.actions[0].cost = cost.map(String::from);
        let report = evaluate(&json!({}), &actions);
        assert_eq!(report.actions[0].total, Some(0.0), "{cost:?}");
        assert_eq!(report.actions[0].rules[0].score, None);
        assert_eq!(report.actions[0].rules[0].reason.code, "invalid_hp_loss");
    }
    actions.actions[0].cost = Some("4503599627370495".into());
    assert_eq!(evaluate(&json!({}), &actions).actions[0].total, Some(-9_007_199_254_740_990.0));
    actions.actions[0].cost_resource = "coin".into();
    actions.actions[0].cost = Some("2".into());
    actions.actions[0].params["hp_cost"] = json!("4503599627370495");
    assert_eq!(evaluate(&json!({}), &actions).actions[0].total, None);
    for cost in [Value::Null, json!(-1), json!("bad"), json!(1.5), json!(true)] {
        actions.actions[0].params["hp_cost"] = cost.clone();
        let report = evaluate(&json!({}), &actions);
        assert_eq!(report.actions[0].total, Some(-2.0), "{cost}");
        assert_eq!(report.actions[0].rules.last().unwrap().reason.code, "invalid_hp_loss");
    }
}

#[test]
fn forest_witch_damage_only_applies_when_entering_a_battle() {
    let mut actions = candidate("next_node", Value::Null, Availability::Available);
    actions.stage = 3;
    for id in [3620, 3621] {
        let mut state = json!({"source":"packet","stage":3,"provider_state":{
            "effect":{"effectList":[{"id":id}]},"character":{"hp":"20"},
            "map":{"level":13001,"node":1}}});
        assert_eq!(evaluate(&state, &actions).actions[0].total, Some(-38.0));
        state["provider_state"]["character"]["hp"] = json!(1);
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
        state["provider_state"]["character"]["hp"] = json!(20);
        state["provider_state"]["map"]["node"] = json!(2);
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
        state["provider_state"]["map"]["node"] = json!(1);
        state["stage"] = json!(9);
        actions.stage = 9;
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
        actions.stage = 3;
    }
}

#[test]
fn gamble_without_outcome_data_keeps_its_known_coin_cost() {
    let mut actions = candidate("gamble", Value::Null, Availability::Available);
    actions.actions[0].cost = Some("5".into());
    let report = evaluate(&json!({}), &actions);
    assert_eq!(report.actions[0].total, Some(-5.0));
    assert_eq!(report.actions[0].rules[0].score, Some(-5.0));
    assert_eq!(report.actions[0].rules[1].rule_id, "gamble_outcomes");
    assert_eq!(report.actions[0].rules[1].score, None);
    assert_eq!(report.actions[0].rules[1].reason.code, "missing_gamble_data");
}

#[test]
fn gamble_uses_every_slot_and_caps_percentage_changes_at_current_hp() {
    let mut actions = candidate("gamble", Value::Null, Availability::Available);
    actions.actions[0].cost = Some("5".into());
    for (hp, expected) in [(100, -35.0 / 3.0), (50, 65.0 / 3.0), (10, 130.0 / 3.0)] {
        let state = json!({"provider_state":{
            "game":{"coin":"50"}, "character":{"hp":hp,"maxHp":"100"},
            "nodeEvent":{"gambleId":2,"gambleCount":0,"gamblePrice":5,"gambleSelections":[6,26,13,11,11,42]}
        }});
        let report = evaluate(&state, &actions);
        assert!((report.actions[0].total.unwrap() - expected).abs() < 1e-10, "hp {hp}");
        assert_eq!(report.actions[0].rules.len(), 2);
        assert_eq!(report.actions[0].rules[0].score, Some(-5.0));
        assert_eq!(report.actions[0].rules[1].reason.code, "gamble_outcomes");
    }
}

#[test]
fn gamble_caps_coin_losses_after_payment_and_ignores_only_missing_effects() {
    let mut actions = candidate("gamble", Value::Null, Availability::Available);
    actions.actions[0].cost = Some("5".into());
    let mut state = json!({"provider_state":{"game":{"coin":"12"},"nodeEvent":{"gambleSelections":[6,39,56]}}});
    // +10, lose min(10, 12-5), lose all remaining 7; then subtract the 5-coin price once.
    let report = evaluate(&state, &actions);
    assert!((report.actions[0].total.unwrap() + 19.0 / 3.0).abs() < 1e-10);
    state["provider_state"]["nodeEvent"]["gambleSelections"] = json!([6,42,9999]);
    let report = evaluate(&state, &actions);
    assert!((report.actions[0].total.unwrap() + 5.0 / 3.0).abs() < 1e-10);
    assert_eq!(report.actions[0].rules[1].reason.code, "partial_gamble_data");
    state["provider_state"]["nodeEvent"]["gambleSelections"] = json!([13,26]);
    assert_eq!(evaluate(&state, &actions).actions[0].total, Some(-5.0));
    for selections in [json!([]), Value::Null, json!({})] {
        state["provider_state"]["nodeEvent"]["gambleSelections"] = selections;
        assert_eq!(evaluate(&state, &actions).actions[0].total, Some(-5.0));
    }
}

#[test]
fn happiness_selection_adds_twenty_to_other_rules_and_the_skip_penalty() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let registry = json!({"amulets":[{"id":302,"sell_price":3}]});
    let book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
        "targets":[{"kind":"amulet","id":302,"value":2},{"kind":"badge","id":600110,"value":3}]}]});
    for effect in [json!({"id":3020,"badgeId":600110}), json!({"id":3020,"badge":{"id":600110}})] {
        let mut actions = candidate("select_candidate", json!(3020), Availability::Available);
        actions.actions[0].params["effect"] = effect;
        actions.actions[0].cost = Some("5".into());
        let mut skip = candidate("skip", Value::Null, Availability::Available).actions.remove(0);
        skip.id = "skip".into();
        actions.actions.push(skip);
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(513.0)); // 20 happiness + 3 price - 5 cost + 495 target
        assert_eq!(report.actions[1].total, Some(-513.0));
        let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "happiness").unwrap();
        assert_eq!(rule.score, Some(20.0));
        assert_eq!(rule.reason.code, "select_happiness");
        actions.actions.truncate(1);
        actions.actions[0].kind = "sell_effect".into();
        actions.actions[0].cost = None;
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(-502.0)); // 5 sale + 3 price - 15 with no packs - 495 target
    }
}

#[test]
fn happiness_sale_uses_total_unsold_pack_cost_with_a_positive_score_at_fifteen() {
    let mut actions = candidate("sell_effect", json!(3020), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600110);
    for (prices, expected) in [
        (vec![], -15.0), (vec![0], -15.0), (vec![5], -10.0), (vec![10], -5.0),
        (vec![14], -1.0), (vec![15], 1.0), (vec![20], 6.0), (vec![5,10,15], 16.0),
    ] {
        let goods: Vec<_> = prices.iter().enumerate().map(|(index, price)|
            json!({"goodsId":101 + index,"price":price.to_string(),"sold":0})).collect();
        let state = json!({"stage":9,"provider_state":{"shop":{"goods":goods}}});
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(expected + 5.0), "prices: {prices:?}");
        assert_eq!(report.actions[0].rules[0].rule_id, "happiness");
        assert_eq!(report.actions[0].rules[0].score, Some(expected));
        assert_eq!(report.actions[0].rules[0].reason.code, "sell_happiness");
    }
}

#[test]
fn happiness_counts_unaffordable_packs_and_keeps_shop_value_during_selection() {
    let mut goods: Vec<_> = [101,102,103,111,112,113].into_iter().enumerate().map(|(index, id)|
        json!({"id":index + 1,"goodsId":id,"price":index + 1,"sold":false})).collect();
    goods.extend([json!({"id":7,"goodsId":103,"price":999,"sold":true}),
        json!({"id":8,"goodsId":111,"price":999,"sold":1}),
        json!({"id":9,"goodsId":201,"price":999,"sold":false})]);
    let mut state = json!({"source":"packet","stage":9,"provider_state":{
        "state":{"current":9,"interrupt":0},"game":{"coin":"0"},
        "effect":{"effectList":[{"id":3020,"uid":1,"volume":1,"badge":{"id":600110}}],
            "shopBuffList":[],"maxEffectVolume":8,"packCandidates":[],"packDisableSkip":false},
        "shop":{"goods":goods,"upgradeGoods":[],"refreshPrice":0}
    }});
    for interrupt in [0,1000] {
        state["provider_state"]["state"]["interrupt"] = json!(interrupt);
        let actions = shanten_backend::action_provider::evaluate(&state);
        assert!(actions.actions.iter().filter(|action| action.kind == "buy_pack")
            .all(|action| action.availability == Availability::Unavailable));
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect").unwrap();
        let report = evaluate(&state, &actions);
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(12.0)); // 5 sale + 1 + 2 + 3 + 4 + 5 + 6 - 14
    }
}

#[test]
fn happiness_ignores_unrelated_actions_and_reports_invalid_pack_data() {
    let mut actions = candidate("sell_effect", json!(3020), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600110);
    for goods in [Value::Null, json!({}), json!([{"goodsId":101,"price":5}]),
        json!([{"goodsId":101,"price":5,"sold":"false"}]), json!([{"goodsId":101,"sold":false}]),
        json!([{"goodsId":101,"price":-1,"sold":false}]), json!([{"goodsId":101,"price":1.5,"sold":false}]),
        json!([{"goodsId":101,"price":"oops","sold":false}]),
        json!([{"goodsId":101,"price":u64::MAX,"sold":false}]),
        json!([{"goodsId":101,"price":9_007_199_254_740_991u64,"sold":false},{"goodsId":102,"price":1,"sold":false}])] {
        let state = json!({"stage":9,"provider_state":{"shop":{"goods":goods}}});
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(5.0), "{goods}");
        assert_eq!(report.actions[0].rules[0].score, None);
        assert_eq!(report.actions[0].rules[0].reason.code, "missing_happiness_pack_cost");
    }
    let state = json!({"stage":9});
    assert_eq!(evaluate(&state, &actions).actions[0].rules[0].score, None);
    for kind in ["buy_effect", "buy_pack", "forge_effect", "skip"] {
        actions.actions[0].kind = kind.into();
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
    }
    for kind in ["select_candidate", "sell_effect"] {
        actions.actions[0].kind = kind.into();
        for badge in [json!(0), json!(600050), json!("600110"), Value::Null] {
            actions.actions[0].params["effect"]["badgeId"] = badge;
            assert!(evaluate(&state, &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "happiness"));
        }
        actions.actions[0].params["effect"]["badgeId"] = json!(600110);
        for availability in [Availability::Unknown, Availability::Unavailable] {
            actions.actions[0].availability = availability;
            assert!(evaluate(&state, &actions).actions.is_empty());
        }
        actions.actions[0].availability = Availability::Available;
    }
}

#[test]
fn guidance_has_nonlinear_diminishing_returns_and_keeps_fractional_scores() {
    let mut actions = candidate("select_candidate", json!(90), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    let mut previous = f64::INFINITY;
    for (count, expected) in [30.0, 28.0, 26.0, 20.0, 10.0, 6.0, 3.0, 1.5, 0.75, 0.375].into_iter().enumerate() {
        let owned: Vec<_> = (0..count).map(|index| if index % 2 == 0 {
            json!({"id":3020,"badge":{"id":600070},"volume":2})
        } else { json!({"id":3030,"badgeId":600070}) }).collect();
        let state = json!({"provider_state":{"effect":{"effectList":owned}}});
        let report = evaluate(&state, &actions);
        let score = &report.actions[0];
        assert_eq!(score.rules.len(), 1);
        assert_eq!(score.rules[0].rule_id, "guidance");
        let total = score.total.unwrap();
        assert!((total - expected).abs() < 1e-12, "owned guidance count: {count}");
        assert!(total > 0.0 && total < previous);
        previous = total;

        let mut sale = candidate("sell_effect", json!(3020), Availability::Available);
        if count % 2 == 0 { sale.actions[0].params["effect"]["badge"] = json!({"id":600070}); }
        else { sale.actions[0].params["effect"]["badgeId"] = json!(600070); }
        let mut after_selection = state.clone();
        after_selection["provider_state"]["effect"]["effectList"].as_array_mut().unwrap()
            .push(sale.actions[0].params["effect"].clone());
        let report = evaluate(&after_selection, &sale);
        assert_eq!(report.actions[0].total, Some(5.0 - expected), "owned guidance count: {}", count + 1);
        assert_eq!(report.actions[0].rules.len(), 2);
        assert_eq!(report.actions[0].rules[0].rule_id, "guidance");
        assert_eq!(report.actions[0].rules[0].score, Some(-expected));
        assert_eq!(report.actions[0].rules[0].reason.code, "sell_guidance");
    }
    let state = json!({"provider_state":{"effect":{"effectList":[
        {"id":3020}, {"id":3030,"badge":{"id":600170}}, {"id":3040,"badgeId":0},
        {"id":3050,"badge":null}, {"id":3060,"badge":{}}
    ]}}});
    assert_eq!(evaluate(&state, &actions).actions[0].total, Some(30.0));
}

#[test]
fn guidance_requires_a_matching_selection_or_sale_and_known_owned_seals() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    for kind in ["buy_effect", "forge_effect", "skip"] {
        let mut actions = candidate(kind, json!(3020), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(600070);
        assert!(evaluate(&state, &actions).actions[0].rules.is_empty());
    }
    for kind in ["select_candidate", "sell_effect"] {
        let mut actions = candidate(kind, json!(3020), Availability::Available);
        for badge in [json!(0), json!(600170), json!("600070"), Value::Null] {
            actions.actions[0].params["effect"]["badgeId"] = badge;
            assert!(evaluate(&state, &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "guidance"));
        }
        actions.actions[0].params["effect"] = json!({"id":3020,"badge":{"id":600070}});
        for owned in [Value::Null, json!({}), json!([false]), json!([{"id":3020,"badge":{"id":"600070"}}]),
            json!([{"id":3020,"badge":false}]), json!([{"id":3020,"badge":{"uid":3}}])] {
            let state = json!({"provider_state":{"effect":{"effectList":owned}}});
            let report = evaluate(&state, &actions);
            assert_eq!(report.actions[0].total, Some(if kind == "sell_effect" { 5.0 } else { 0.0 }));
            assert_eq!(report.actions[0].rules[0].score, None);
            assert_eq!(report.actions[0].rules[0].reason.code, "missing_guidance_count");
        }
        assert_eq!(evaluate(&json!({"effect_list":[]}), &actions).actions[0].total, Some(if kind == "sell_effect" { 5.0 } else { 0.0 }));
        if kind == "sell_effect" {
            let report = evaluate(&state, &actions);
            assert_eq!(report.actions[0].total, Some(5.0));
            assert_eq!(report.actions[0].rules[0].score, None);
            assert_eq!(report.actions[0].rules[0].reason.code, "missing_guidance_count");
        }
        for availability in [Availability::Unknown, Availability::Unavailable] {
            actions.actions[0].availability = availability;
            assert!(evaluate(&state, &actions).actions.is_empty());
        }
    }
}

#[test]
fn fractional_guidance_accumulates_with_other_rules_and_survives_serialization() {
    let owned: Vec<_> = (0..8).map(|_| json!({"id":3020,"badge":{"id":600070}})).collect();
    let state = json!({"provider_state":{"effect":{"effectList":owned}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    actions.actions[0].cost = Some("10".into());
    actions.actions[0].params["hp_cost"] = json!(7);
    let report = evaluate(&state, &actions);
    assert_eq!(report.actions[0].total, Some(26.75));
    assert_eq!(report.actions[0].rules.iter().map(|rule| rule.score).collect::<Vec<_>>(),
        vec![Some(50.0), Some(-10.0), Some(-14.0), Some(0.75)]);
    let round_trip: decision::DecisionReport = serde_json::from_value(serde_json::to_value(report).unwrap()).unwrap();
    assert_eq!(round_trip.actions[0].total, Some(26.75));
}

#[test]
fn amulet_price_uses_variant_prices_and_only_multiplies_gold_candidates() {
    let registry = json!({"amulets":[{"id":302,"sell_price":3,"plus_sell_price":9}]});
    for source in ["packet", "qyzz"] {
        let state = json!({"source":source,"provider_state":{"effect":{"effectList":[
            {"id":3030,"badge":{"id":600050}}
        ]}}});
        for (id, price) in [(3020, 3.0), (3021, 9.0)] {
            for (badge, multiplier, reason) in [
                (json!({}), 1.0, "amulet_price"),
                (json!({"badgeId":0}), 1.0, "amulet_price"),
                (json!({"badge":{"id":600170}}), 1.0, "amulet_price"),
                (json!({"badgeId":600050}), if source == "qyzz" { 3.0 } else { 2.0 },
                    if source == "qyzz" { "amulet_price_gold_qyzz" } else { "amulet_price_gold" }),
                (json!({"badge":{"id":600050}}), if source == "qyzz" { 3.0 } else { 2.0 },
                    if source == "qyzz" { "amulet_price_gold_qyzz" } else { "amulet_price_gold" }),
            ] {
                let mut actions = candidate("select_candidate", json!(id), Availability::Available);
                actions.actions[0].params["effect"].as_object_mut().unwrap().extend(badge.as_object().unwrap().clone());
                let report = evaluate_with_registry(&state, &actions, &registry);
                assert_eq!(report.actions[0].total, Some(price * multiplier), "{source}: {id}: {badge}");
                assert_eq!(report.actions[0].rules.len(), 1);
                assert_eq!(report.actions[0].rules[0].rule_id, "amulet_price");
                assert_eq!(report.actions[0].rules[0].reason.code, reason);
            }
        }
    }
    for kind in ["buy_effect", "sell_effect", "forge_effect", "skip"] {
        assert!(evaluate_with_registry(&json!({}), &candidate(kind, json!(3020), Availability::Available), &registry)
            .actions[0].rules.iter().all(|rule| rule.rule_id != "amulet_price"));
    }
    for availability in [Availability::Unknown, Availability::Unavailable] {
        assert!(evaluate_with_registry(&json!({}), &candidate("select_candidate", json!(3020), availability), &registry)
            .actions.is_empty());
    }
}

#[test]
fn amulet_price_ignores_invalid_data_and_preserves_other_scores() {
    let state = json!({"source":"qyzz","provider_state":{"effect":{"effectList":[]}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600050);
    for price in [Value::Null, json!(-1), json!(1.5), json!("9"), json!(u64::MAX), json!(3_002_399_751_580_331u64)] {
        let registry = json!({"amulets":[{"id":224,"sell_price":price}]});
        let report = evaluate_with_registry(&state, &actions, &registry);
        assert_eq!(report.actions[0].total, Some(50.0), "{price}");
        assert_eq!(report.actions[0].rules.last().unwrap().score, None);
        assert_eq!(report.actions[0].rules.last().unwrap().reason.code, "missing_amulet_price");
    }
    assert_eq!(evaluate_with_registry(&state, &actions, &json!({"amulets":[]})).actions[0].total, Some(50.0));
    let registry = json!({"amulets":[{"id":224,"sell_price":9}]});
    for badge in [json!("600050"), json!(-1), Value::Null] {
        actions.actions[0].params["effect"]["badgeId"] = badge;
        assert_eq!(evaluate_with_registry(&state, &actions, &registry).actions[0].total, Some(50.0));
    }
    actions.actions[0].params["effect"]["badgeId"] = json!(0);
    actions.actions[0].params["effect"]["id"] = json!(2241);
    assert_eq!(evaluate_with_registry(&state, &actions, &registry).actions[0].total, Some(50.0));
}

#[test]
fn amulet_price_accumulates_with_guidance_and_costs() {
    let state = json!({"provider_state":{"effect":{"effectList":[
        {"id":3020,"badge":{"id":600070}}, {"id":3030,"badge":{"id":600070}}
    ]}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    actions.actions[0].cost = Some("10".into());
    actions.actions[0].params["hp_cost"] = json!(7);
    let registry = json!({"amulets":[{"id":224,"sell_price":9}]});
    let report = evaluate_with_registry(&state, &actions, &registry);
    assert_eq!(report.actions[0].total, Some(61.0)); // 50 + 26 + 9 - 10 - 14
    assert_eq!(report.actions[0].rules.iter().map(|rule| rule.score).collect::<Vec<_>>(),
        vec![Some(50.0), Some(-10.0), Some(-14.0), Some(26.0), Some(9.0)]);
}

#[test]
fn sale_price_scores_normal_plus_and_gold_amulets_using_the_displayed_price() {
    for source in ["packet", "qyzz"] {
        for registry in [
            json!({"amulets":[{"id":302,"sell_price":3,"plus_sell_price":9,"metadata":{"sell_price":99}}]}),
            json!({"amulets":[{"id":302,"metadata":{"sell_price":3},"plus_metadata":{"sell_price":9}}]}),
        ] {
            for (id, price) in [(3020,3.0), (3021,9.0)] {
                for (badge, multiplier) in [
                    (json!({}), 1.0), (json!({"badgeId":0}), 1.0),
                    (json!({"badge":{"id":600170}}), 1.0),
                    (json!({"badgeId":600050}), 3.0), (json!({"badge":{"id":600050}}), 3.0),
                ] {
                    let mut actions = candidate("sell_effect", json!(id), Availability::Available);
                    actions.actions[0].params["effect"].as_object_mut().unwrap().extend(badge.as_object().unwrap().clone());
                    let report = evaluate_with_registry(&json!({"source":source}), &actions, &registry);
                    assert_eq!(report.actions[0].total, Some(5.0 + price * multiplier), "{source}, {id}, {badge}");
                    let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "sell_amulet_price").unwrap();
                    assert_eq!(rule.score, Some(price * multiplier));
                    assert_eq!(rule.reason.code, "sell_amulet_price");
                    for availability in [Availability::Unavailable, Availability::Unknown] {
                        actions.actions[0].availability = availability;
                        assert!(evaluate_with_registry(&json!({}), &actions, &registry).actions.is_empty());
                    }
                }
            }
        }
    }
    for kind in ["select_candidate", "buy_effect", "buy_pack", "forge_effect", "skip"] {
        let report = evaluate(&json!({}), &candidate(kind, json!(3020), Availability::Available));
        assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "sell_amulet_price"));
    }
}

#[test]
fn sale_price_ignores_invalid_prices_and_seals_while_preserving_the_sale_bonus() {
    let mut actions = candidate("sell_effect", json!(3020), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600050);
    for price in [Value::Null, json!(-1), json!(1.5), json!("9"), json!(u64::MAX), json!(3_002_399_751_580_331u64)] {
        let registry = json!({"amulets":[{"id":302,"sell_price":price}]});
        let report = evaluate_with_registry(&json!({}), &actions, &registry);
        assert_eq!(report.actions[0].total, Some(5.0), "{price}");
        assert_eq!(report.actions[0].rules.last().unwrap().score, None);
        assert_eq!(report.actions[0].rules.last().unwrap().reason.code, "missing_amulet_price");
    }
    assert_eq!(evaluate_with_registry(&json!({}), &actions, &json!({})).actions[0].total, Some(5.0));
    let registry = json!({"amulets":[{"id":302,"sell_price":9}]});
    for badge in [Value::Null, json!("600050"), json!(-1)] {
        actions.actions[0].params["effect"]["badgeId"] = badge;
        assert_eq!(evaluate_with_registry(&json!({}), &actions, &registry).actions[0].total, Some(5.0));
    }
    actions.actions[0].params["effect"]["badgeId"] = json!(0);
    actions.actions[0].params["effect"]["id"] = json!(3021);
    assert_eq!(evaluate_with_registry(&json!({}), &actions, &registry).actions[0].total, Some(5.0));
    let free = json!({"amulets":[{"id":302,"plus_sell_price":0}]});
    assert_eq!(evaluate_with_registry(&json!({}), &actions, &free).actions[0].total, Some(5.0));
}

#[test]
fn duplicate_selection_deducts_the_screenshot_losses_and_updates_skip() {
    let state = json!({"source":"packet","stage":16,"provider_state":{
        "state":{"current":16,"interrupt":0},"effect":{
            "maxEffectVolume":8,"packDisableSkip":false,
            "effectList":[
                {"id":90,"uid":11,"volume":1,"badge":{"id":600070}},
                {"id":10,"uid":3,"volume":1,"badge":{"id":600070}},
                {"id":20,"uid":5,"volume":1,"badge":{"id":600070}},
                {"id":30,"uid":8,"volume":1,"badge":{"id":600070}}
            ],"packCandidates":[{"id":3540,"badgeId":0},{"id":90,"badgeId":600050},{"id":3200,"badgeId":0}]
        }
    }});
    let registry = json!({"amulets":[
        {"id":9,"sell_price":9,"plus_sell_price":9},
        {"id":354,"sell_price":9},{"id":320,"sell_price":3}
    ]});
    let actions = shanten_backend::action_provider::evaluate(&state);
    let report = evaluate_with_registry(&state, &actions, &registry);
    for (index, expected) in [9.0,-11.0,3.0,-9.0].into_iter().enumerate() {
        let action = &actions.actions[index];
        let score = report.actions.iter().find(|score| score.action_id == action.id).unwrap();
        assert_eq!(score.total, Some(expected));
    }
    let score = &report.actions[1];
    assert_eq!(score.rules.iter().map(|rule| (rule.rule_id.as_str(), rule.score)).collect::<Vec<_>>(),
        vec![("amulet_price",Some(18.0)), ("replacement_amulet",Some(-9.0)), ("replacement_guidance",Some(-20.0))]);
    assert_eq!(actions.actions[3].kind, "skip");
    let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 11).unwrap();
    let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
    assert_eq!(score.total, Some(3.0)); // 5 sale + 9 price - 20 guidance + (18 after - 9 before).
    assert_eq!(score.rules.iter().find(|rule| rule.rule_id == "sale_followup").unwrap().score, Some(9.0));
    assert_eq!(state["provider_state"]["effect"]["effectList"].as_array().unwrap().len(), 4);
    let mut after = state.clone();
    after["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().remove(0);
    let after_actions = shanten_backend::action_provider::evaluate(&after);
    let after_scores = evaluate_with_registry(&after, &after_actions, &registry);
    assert_eq!(after_scores.actions[1].total, Some(18.0));
    assert_eq!(after_scores.actions[3].total, Some(-18.0));

    let mut more = state.clone();
    more["provider_state"]["effect"]["effectList"].as_array_mut().unwrap()
        .push(json!({"id":90,"uid":12,"volume":1,"badge":{"id":600070}}));
    let report = evaluate_with_registry(&more, &shanten_backend::action_provider::evaluate(&more), &registry);
    let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
    assert_eq!(score.total, Some(4.0)); // 9 price + 5 sale - 10 for the fifth seal; another duplicate keeps the best choice at 9.
    assert!(score.rules.iter().all(|rule| rule.rule_id != "sale_followup"));

    let mut valuable = registry.clone();
    valuable["amulets"][1]["sell_price"] = json!(100);
    let report = evaluate_with_registry(&state, &actions, &valuable);
    assert_eq!(report.actions[0].total, Some(100.0));
    assert_eq!(report.actions.iter().find(|score| score.action_id == sale.id).unwrap().total, Some(-6.0));
}

#[test]
fn sale_followup_rechecks_capacity_and_only_uses_legal_choices() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9},{"id":354,"sell_price":9}]});
    for (maximum, can_skip, expected_followup) in [(3,false,Some(9.0)), (2,false,Some(0.0)), (2,true,Some(0.0))] {
        let state = json!({"source":"packet","stage":16,"provider_state":{
            "state":{"interrupt":0},"effect":{"maxEffectVolume":maximum,"packDisableSkip":!can_skip,
                "effectList":[{"id":90,"uid":1,"volume":1},{"id":100,"uid":2,"volume":1}],
                "packCandidates":[{"id":3540,"badgeId":600160}]}
        }});
        let actions = shanten_backend::action_provider::evaluate(&state);
        assert_eq!(actions.actions[0].availability, Availability::Unavailable);
        let report = evaluate_with_registry(&state, &actions, &registry);
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 1).unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(14.0 + expected_followup.unwrap_or(0.0)));
        let future = score.rules.iter().find(|rule| rule.rule_id == "sale_followup");
        if expected_followup == Some(0.0) { assert!(future.is_none()); }
        else { assert_eq!(future.unwrap().score, expected_followup); }
    }
}

#[test]
fn sale_followup_does_not_reward_unchanged_negative_or_skippable_choices() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9},{"id":10,"sell_price":9}]});
    for can_skip in [true, false] {
        let state = json!({"source":"packet","stage":16,"provider_state":{
            "state":{"interrupt":0},"effect":{"maxEffectVolume":8,"packDisableSkip":!can_skip,
                "effectList":[{"id":90,"uid":1,"volume":1,"badgeId":600070},
                    {"id":90,"uid":2,"volume":1,"badgeId":600070},{"id":100,"uid":3,"volume":1}],
                "packCandidates":[{"id":90,"badgeId":0}]}
        }});
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 3).unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(14.0));
        assert!(score.rules.iter().all(|rule| rule.rule_id != "sale_followup"));
    }
}

#[test]
fn sale_followup_requires_unique_identity_and_a_selection_stage() {
    let mut state = json!({"source":"packet","stage":16,"provider_state":{
        "state":{"interrupt":0},"effect":{"maxEffectVolume":8,"packDisableSkip":false,
            "effectList":[{"id":90,"uid":1,"volume":1}],"packCandidates":[{"id":90,"badgeId":600050}]},
        "shop":{"goods":[],"upgradeGoods":[],"refreshPrice":1}
    }});
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    let actions = shanten_backend::action_provider::evaluate(&state);
    for uid in [Value::Null,json!(99)] {
        let mut actions = actions.clone();
        let sale = actions.actions.iter_mut().find(|action| action.kind == "sell_effect").unwrap();
        sale.params["uid"] = uid;
        let sale_id = sale.id.clone();
        let report = evaluate_with_registry(&state, &actions, &registry);
        let score = report.actions.iter().find(|score| score.action_id == sale_id).unwrap();
        assert_eq!(score.total, Some(14.0));
        assert_eq!(score.rules.last().unwrap().reason.code, "missing_sale_followup");
    }
    state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().push(json!({"id":100,"uid":1,"volume":1}));
    let report = evaluate_with_registry(&state, &actions, &registry);
    let sale = actions.actions.iter().find(|action| action.kind == "sell_effect").unwrap();
    let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
    assert_eq!(score.total, Some(14.0));
    assert_eq!(score.rules.last().unwrap().reason.code, "missing_sale_followup");
    state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().pop();
    for stage in [9,13] {
        state["stage"] = json!(stage);
        let actions = shanten_backend::action_provider::evaluate(&state);
        assert!(evaluate_with_registry(&state, &actions, &registry).actions.iter()
            .all(|score| score.rules.iter().all(|rule| rule.rule_id != "sale_followup")));
    }
    state["source"] = json!("qyzz");
    assert!(shanten_backend::action_provider::evaluate(&state).actions.is_empty());
}

#[test]
fn sale_followup_does_not_guess_when_either_best_choice_is_unknown() {
    for old_id in [90, 91] {
        let mut candidates = vec![json!({"id":90,"badgeId":600050})];
        if old_id == 90 { candidates.push(json!({"id":3540,"badgeId":0})); }
        let state = json!({"source":"packet","stage":16,"provider_state":{
            "state":{"interrupt":0},"effect":{"maxEffectVolume":1,"packDisableSkip":false,
                "effectList":[{"id":old_id,"uid":1,"volume":1,"badgeId":0}],"packCandidates":candidates}
        }});
        // Missing PLUS price makes the current replacement unknown; missing 354 price affects only the freed slot.
        let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect").unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        let followup = score.rules.iter().find(|rule| rule.rule_id == "sale_followup").unwrap();
        assert_eq!(followup.score, None, "old id {old_id}");
        assert_eq!(followup.reason.code, "missing_sale_followup");
    }
}

#[test]
fn duplicate_selection_matches_one_consumed_card_using_upgrade_metadata() {
    for (id, owned, expected, lost_seal) in [
        (90,json!([{"id":91,"badgeId":600110},{"id":90,"badgeId":600070}]),-30.0,"replacement_guidance"),
        (90,json!([{"id":90,"badgeId":600110},{"id":90,"badgeId":600070}]),-20.0,"replacement_happiness"),
        (90,json!([{"id":91,"badgeId":600070}]),-33.0,"replacement_guidance"),
        (91,json!([{"id":90,"badgeId":600070}]),12.0,""),
        (1340,json!([{"id":1341,"badgeId":600070}]),9.0,""),
        (1340,json!([{"id":1340,"badgeId":600070}]),-30.0,"replacement_guidance"),
        (1291,json!([{"id":1291,"badgeId":600070}]),-30.0,"replacement_guidance"),
        (90,json!([{"id":100,"badgeId":600070}]),9.0,""),
    ] {
        let state = json!({"provider_state":{"effect":{"effectList":owned}}});
        let registry = json!({"amulets":[{"id":id / 10,"sell_price":9,"plus_sell_price":12}]});
        let actions = candidate("select_candidate", json!(id), Availability::Available);
        let report = evaluate_with_registry(&state, &actions, &registry);
        assert_eq!(report.actions[0].total, Some(expected), "{id}: {owned}");
        let losses: Vec<_> = report.actions[0].rules.iter().filter(|rule| rule.rule_id.starts_with("replacement_")).collect();
        assert_eq!(losses.len(), if lost_seal.is_empty() { 0 } else { 2 });
        if !lost_seal.is_empty() { assert_eq!(losses[1].rule_id, lost_seal); }
    }
}

#[test]
fn replacing_guidance_with_guidance_has_zero_net_seal_value() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    for (count, last_value) in [(1,30.0),(4,20.0),(9,0.75)] {
        let mut owned = vec![json!({"id":90,"badgeId":600070})];
        owned.extend((1..count).map(|_| json!({"id":100,"badgeId":600070})));
        let state = json!({"provider_state":{"effect":{"effectList":owned}}});
        let mut actions = candidate("select_candidate", json!(90), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(600070);
        let report = evaluate_with_registry(&state, &actions, &registry);
        assert_eq!(report.actions[0].total, Some(0.0));
        for (id, expected) in [("guidance",last_value), ("replacement_guidance",-last_value)] {
            assert_eq!(report.actions[0].rules.iter().find(|rule| rule.rule_id == id).unwrap().score, Some(expected));
        }
    }
}

#[test]
fn duplicate_selection_loses_happiness_and_target_value_without_sale_rewards() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    let state = json!({"stage":9,"provider_state":{
        "effect":{"effectList":[{"id":90,"badgeId":600110}]},
        "shop":{"goods":[{"goodsId":103,"price":100,"sold":false}]}
    }});
    let book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
        "targets":[{"kind":"amulet","id":9,"value":2},{"kind":"badge","id":600110,"value":3}]}]});
    for (badge, expected) in [(0,-515.0), (600110,-198.0)] {
        let mut actions = candidate("select_candidate", json!(90), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(badge);
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(expected));
        for (id, score) in [("replacement_happiness",-20.0), ("replacement_target",-495.0)] {
            assert_eq!(report.actions[0].rules.iter().find(|rule| rule.rule_id == id).unwrap().score, Some(score));
        }
        assert!(report.actions[0].rules.iter().all(|rule| !matches!(rule.rule_id.as_str(), "sell_bonus" | "sell_amulet_price")));
    }
    let state = json!({"provider_state":{"effect":{"effectList":[{"id":90,"badgeId":600050}]}}});
    let report = evaluate_with_registry(&state, &candidate("select_candidate", json!(90), Availability::Available), &registry);
    assert_eq!(report.actions[0].total, Some(-18.0)); // 9 gained - 27 forfeited sale price
}

#[test]
fn duplicate_selection_values_targets_using_the_resulting_plus_variant() {
    let registry = json!({"amulets":[{"id":9,"sell_price":9,"plus_sell_price":9}]});
    for (old_id, target_plus, expected) in [(90,false,-198.0), (90,true,198.0), (91,true,0.0)] {
        let state = json!({"provider_state":{"effect":{"effectList":[{"id":old_id,"badgeId":0}]}}});
        let book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
            "targets":[{"kind":"amulet","id":9,"plus":target_plus,"value":2}]}]});
        let actions = candidate("select_candidate", json!(90), Availability::Available);
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(expected), "owned {old_id}, target plus {target_plus}");
    }
}

#[test]
fn duplicate_selection_keeps_known_losses_when_data_is_missing_and_marks_group_merges_unknown() {
    let actions = candidate("select_candidate", json!(90), Availability::Available);
    let state = json!({"provider_state":{"effect":{"effectList":[{"id":90,"badgeId":600070}]}}});
    let report = evaluate_with_registry(&state, &actions, &json!({}));
    assert_eq!(report.actions[0].total, Some(-30.0));
    assert_eq!(report.actions[0].rules.iter().find(|rule| rule.rule_id == "replacement_amulet").unwrap().score, None);
    let registry = json!({"amulets":[{"id":9,"sell_price":9}]});
    for owned in [Value::Null, json!([{}]), json!([{"id":"90"}])] {
        let state = json!({"provider_state":{"effect":{"effectList":owned}}});
        let report = evaluate_with_registry(&state, &actions, &registry);
        assert_eq!(report.actions[0].total, Some(9.0));
        let loss = report.actions[0].rules.iter().find(|rule| rule.rule_id == "replacement_amulet").unwrap();
        assert_eq!(loss.score, None);
        assert_eq!(loss.reason.code, "missing_effects");
    }
    for availability in [Availability::Unavailable, Availability::Unknown] {
        assert!(evaluate_with_registry(&state, &candidate("select_candidate", json!(90), availability), &registry).actions.is_empty());
    }
    let owned: Vec<_> = [2200,2250,3020,3030,3040,3200,3240,3250,3510,3520,3530,3730,4050]
        .into_iter().map(|id| json!({"id":id,"badgeId":600070})).collect();
    let state = json!({"provider_state":{"effect":{"effectList":owned}}});
    let mut actions = candidate("select_candidate", json!(3020), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    let report = evaluate(&state, &actions);
    assert_eq!(report.actions[0].total, Some(0.0));
    assert!(report.actions[0].rules.iter().all(|rule| rule.score.is_none() && rule.reason.code == "unknown_replacement_group"));
}

#[test]
fn pack_value_scores_one_guaranteed_and_two_random_candidates_with_guidance() {
    for (goods_id, expected) in [
        (101, [4.4898, 4.985938056247775, 6.0667273673257025, 7.576505491730842]),
        (102, [6.2925, 6.762396694214876, 7.6186224489795915, 8.488185255198488]),
        (103, [9.1773, 9.383920847750865, 9.627200349368449, 9.794624702439991]),
    ] {
        for (count, expected) in expected.into_iter().enumerate() {
            let mut owned: Vec<_> = (0..count).map(|index| if index % 2 == 0 {
                json!({"id":3020,"badge":{"id":600070},"volume":2})
            } else { json!({"id":3030,"badgeId":600070}) }).collect();
            owned.push(json!({"id":3040,"badge":{"id":600050}}));
            let state = json!({"provider_state":{"effect":{"effectList":owned}}});
            for id in [goods_id, goods_id + 10] {
                let mut actions = candidate("buy_pack", Value::Null, Availability::Available);
                actions.actions[0].params = json!({"goods_id":id});
                let report = evaluate(&state, &actions);
                let bonus = if goods_id == 103 { 5.0 } else { 0.0 };
                assert!((report.actions[0].total.unwrap() - expected - bonus).abs() < 1e-12, "pack {id}, guidance {count}");
                assert!(report.actions[0].total.unwrap() >= f64::from((goods_id - 100) * 3));
                assert_eq!(report.actions[0].rules.len(), if goods_id == 103 { 2 } else { 1 });
                assert!((report.actions[0].rules[0].score.unwrap() - expected).abs() < 1e-12);
                assert_eq!(report.actions[0].rules[0].rule_id, "pack_value");
                assert_eq!(report.actions[0].rules[0].reason.code, "pack_value");
            }
        }
    }
    let owned = vec![json!({"id":3020,"badgeId":600070}); 1000];
    let state = json!({"provider_state":{"effect":{"effectList":owned}}});
    for (id, expected) in [(101, 9.0), (102, 9.0), (103, 14.916666666666666)] {
        let mut actions = candidate("buy_pack", Value::Null, Availability::Available);
        actions.actions[0].params = json!({"goods_id":id});
        assert!((evaluate(&state, &actions).actions[0].total.unwrap() - expected).abs() < 1e-12);
    }
}

#[test]
fn gold_pack_bonus_is_independent_of_expectation_and_stacks_with_payment() {
    for id in [101,102,103,111,112,113] {
        let bonus = if matches!(id, 103 | 113) { 5.0 } else { 0.0 };
        let mut actions = candidate("buy_pack", Value::Null, Availability::Available);
        actions.actions[0].params = json!({"goods_id":id});
        actions.actions[0].cost = Some("3".into());
        for (resource, penalty) in [("coin",3.0), ("hp",6.0)] {
            actions.actions[0].cost_resource = resource.into();
            let report = evaluate(&json!({}), &actions); // Unknown inventory only disables the expectation rule.
            assert_eq!(report.actions[0].total, Some(bonus - penalty));
            let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "gold_pack_bonus");
            assert_eq!(rule.map(|rule| rule.score), (bonus > 0.0).then_some(Some(5.0)));
            if let Some(rule) = rule { assert_eq!(rule.reason.code, "gold_pack_bonus"); }
        }
        for availability in [Availability::Unavailable, Availability::Unknown] {
            actions.actions[0].availability = availability;
            assert!(evaluate(&json!({}), &actions).actions.is_empty());
        }
    }
    let mut actions = candidate("buy_pack", Value::Null, Availability::Available);
    for id in [Value::Null, json!("103"), json!(-103), json!(203), json!(999)] {
        actions.actions[0].params = json!({"goods_id":id});
        assert!(evaluate(&json!({}), &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "gold_pack_bonus"));
    }
    actions.actions[0].params = json!({"goods_id":103});
    for kind in ["buy_effect", "buy_rune", "select_candidate", "skip"] {
        actions.actions[0].kind = kind.into();
        assert!(evaluate(&json!({}), &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "gold_pack_bonus"));
    }
}

#[test]
fn pack_value_requires_a_known_pack_and_inventory_and_adds_to_payment_rules() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let mut actions = candidate("buy_pack", Value::Null, Availability::Available);
    actions.actions[0].cost = Some("5".into());
    actions.actions[0].params = json!({"goods_id":101});
    for (resource, penalty) in [("coin", 5.0), ("hp", 10.0)] {
        actions.actions[0].cost_resource = resource.into();
        assert!((evaluate(&state, &actions).actions[0].total.unwrap() - (4.4898 - penalty)).abs() < 1e-12);
    }
    actions.actions[0].cost_resource = "coin".into();
    for owned in [Value::Null, json!({}), json!([false]), json!([{"id":3020,"badgeId":"600070"}])] {
        let state = json!({"provider_state":{"effect":{"effectList":owned}}});
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(-5.0));
        assert_eq!(report.actions[0].rules.last().unwrap().reason.code, "missing_guidance_count");
    }
    for id in [Value::Null, json!("101"), json!(-1), json!(999)] {
        actions.actions[0].params["goods_id"] = id;
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].total, Some(-5.0));
        assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "pack_value"));
    }
    actions.actions[0].params["goods_id"] = json!(101);
    for availability in [Availability::Unknown, Availability::Unavailable] {
        actions.actions[0].availability = availability;
        assert!(evaluate(&state, &actions).actions.is_empty());
    }
    actions.actions[0].availability = Availability::Available;
    for kind in ["select_candidate", "buy_effect", "buy_rune", "skip", "select_character"] {
        actions.actions[0].kind = kind.into();
        assert!(evaluate(&state, &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "pack_value"));
    }
}

#[test]
fn configured_opening_targets_sum_amulet_and_badge_values_for_selection_and_sale() {
    let book = json!({"selected_profile_id":"opening","profiles":[
        {"id":"ignored","strategy":"opening","targets":[{"kind":"amulet","id":303,"value":100}]},
        {"id":"opening","strategy":"opening","targets":[
            {"kind":"amulet","id":302,"value":2},
            {"kind":"amulet","id":302,"plus":true,"badge":600050,"value":4},
            {"kind":"badge","id":600050,"value":3},
            {"kind":"amulet","id":304}
        ]}
    ]});
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    for (id, badge, value) in [(3020,0,2), (3020,600050,5), (3021,600050,7), (3021,0,0),
        (3021,600170,0), (3030,600050,3), (3030,0,0), (3040,0,1)] {
        for kind in ["select_candidate", "sell_effect"] {
            for nested_badge in [false, true] {
                let mut actions = candidate(kind, json!(id), Availability::Available);
                if nested_badge { actions.actions[0].params["effect"]["badge"] = json!({"id":badge}); }
                else { actions.actions[0].params["effect"]["badgeId"] = json!(badge); }
                let registry = json!({"amulets":[{"id":id / 10,"sell_price":0,"plus_sell_price":0}]});
                let report = decision::evaluate(&state, &actions, &registry, &book);
                let expected = f64::from(if kind == "sell_effect" { 5 - 99 * value } else { 99 * value });
                assert_eq!(report.actions[0].total, Some(expected), "{kind}: {id}: {badge}");
                if value > 0 {
                    let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "configured_target").unwrap();
                    assert_eq!(rule.rule_id, "configured_target");
                    assert_eq!(rule.reason.code, if kind == "sell_effect" { "sell_configured_target" } else { "select_configured_target" });
                } else { assert!(report.actions[0].rules.iter().all(|rule| rule.rule_id != "configured_target")); }
            }
        }
    }
}

#[test]
fn full_targets_use_only_the_current_stage_and_deduplicate_loadouts() {
    let mut book = json!({"selected_profile_id":"full","profiles":[{
        "id":"full","initial_amulet_id":302,"stages":[
            {"start_level":13001,"start_node":3,"loadouts":[
                {"amulets":[{"id":303,"plus":false,"badge":null},{"id":304,"plus":true,"badge":600170}]},
                {"amulets":[{"id":303,"plus":false,"badge":null},{"id":305,"plus":false,"badge":0}]}
            ]},
            {"start_level":13001,"start_node":5,"loadouts":[{"amulets":[{"id":306,"plus":false,"badge":null}]}]},
            {"start_level":13002,"start_node":2,"loadouts":[{"amulets":[{"id":307,"plus":false,"badge":null}]}]}
        ]
    }]});
    for explicit_strategy in [false, true] {
        if explicit_strategy { book["profiles"][0]["strategy"] = json!("full"); }
        for (level, node, id, badge, value) in [
            (13001,2,3020,0,1), (13001,2,3030,0,0),
            (13001,3,3020,0,0), (13001,3,3030,0,1), (13001,4,3030,0,1),
            (13001,3,3040,600170,0), (13001,3,3041,0,0), (13001,3,3041,600170,1),
            (13001,3,3050,0,1), (13001,3,3050,600050,0),
            (13001,4,3060,0,0), (13001,5,3030,0,0), (13001,5,3060,0,1),
            (13002,1,3060,0,1), (13002,2,3060,0,0), (13002,2,3070,0,1)
        ] {
            let state = json!({"stage":16,"provider_state":{"map":{"level":level,"node":node}}});
            for kind in ["select_candidate", "sell_effect"] {
                let mut actions = candidate(kind, json!(id), Availability::Available);
                actions.actions[0].params["effect"]["badgeId"] = json!(badge);
                let registry = json!({"amulets":[{"id":id / 10,"sell_price":0,"plus_sell_price":0}]});
                let report = decision::evaluate(&state, &actions, &registry, &book);
                assert_eq!(report.actions[0].total, Some(f64::from(if kind == "sell_effect" { 5 - 99 * value } else { 99 * value })),
                    "{kind}: {level}:{node}, effect {id}/{badge}");
            }
        }
    }
    let initial = candidate("select_candidate", json!(3020), Availability::Available);
    assert_eq!(decision::evaluate(&json!({"stage":2}), &initial, &json!({}), &book).actions[0].total, Some(99.0));
    for state in [json!({}), json!({"provider_state":{"map":{"level":99999,"node":1}}}),
        json!({"provider_state":{"map":{"level":11001,"node":3}}})] {
        let report = decision::evaluate(&state, &initial, &json!({}), &book);
        assert_eq!(report.actions[0].total, Some(0.0));
        assert_eq!(report.actions[0].rules.iter().find(|rule| rule.rule_id == "configured_target").unwrap().reason.code, "missing_tactical_stage");
    }
}

#[test]
fn configured_target_checks_values_and_accumulates_with_existing_rules() {
    let mut book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
        "targets":[{"kind":"amulet","id":224,"value":2},{"kind":"badge","id":600070,"value":3}]}]});
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let mut actions = candidate("select_candidate", json!(2240), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    actions.actions[0].cost = Some("10".into());
    let registry = json!({"amulets":[{"id":224,"sell_price":9}]});
    let report = decision::evaluate(&state, &actions, &registry, &book);
    assert_eq!(report.actions[0].total, Some(574.0)); // 50 + 30 + 9 - 10 + 99 * (2 + 3)
    let round_trip: decision::DecisionReport = serde_json::from_value(serde_json::to_value(report).unwrap()).unwrap();
    assert_eq!(round_trip.actions[0].total, Some(574.0));
    let mut sale = candidate("sell_effect", json!(2240), Availability::Available);
    sale.actions[0].params["effect"]["badgeId"] = json!(600070);
    let owned = json!({"provider_state":{"effect":{"effectList":[sale.actions[0].params["effect"]]}}});
    let report = decision::evaluate(&owned, &sale, &registry, &book);
    assert_eq!(report.actions[0].total, Some(-561.0)); // 5 sale + 9 price - 50 Ryan - 30 guidance - 99 * (2 + 3)
    assert_eq!(report.actions[0].rules.iter().map(|rule| rule.score).collect::<Vec<_>>(),
        vec![Some(-50.0), Some(-30.0), Some(-495.0), Some(5.0), Some(9.0)]);
    for value in [Value::Null, json!(-1), json!(0), json!(1.5), json!("2"), json!(u64::MAX), json!(9_007_199_254_740_991u64)] {
        book["profiles"][0]["targets"][0]["value"] = value;
        let report = decision::evaluate(&state, &actions, &registry, &book);
        assert_eq!(report.actions[0].total, Some(79.0));
        assert_eq!(report.actions[0].rules.last().unwrap().reason.code, "invalid_target_value");
    }
    for kind in ["buy_effect", "forge_effect", "skip"] {
        actions.actions[0].kind = kind.into();
        assert!(decision::evaluate(&state, &actions, &registry, &book).actions[0].rules.iter()
            .all(|rule| rule.rule_id != "configured_target"));
    }
    actions.actions[0].kind = "select_candidate".into();
    for availability in [Availability::Unknown, Availability::Unavailable] {
        actions.actions[0].availability = availability;
        assert!(decision::evaluate(&state, &actions, &registry, &book).actions.is_empty());
    }
}

#[test]
fn target_scores_refresh_when_the_saved_profile_changes_or_reloads() {
    use shanten_backend::{pipeline::{Direction, Packet}, services::Services};
    let root = std::env::temp_dir().join(format!("lens-target-score-{}", std::process::id()));
    let (events, _) = tokio::sync::broadcast::channel(16);
    let services = Services::load(root.join("configs"), events).unwrap();
    services.update_game_state_from_flow(&Packet {
        direction: Direction::Inbound, packet_type: "Res".into(), id: Some(1),
        method: ".lq.Lobby.fetchAmuletActivityData".into(),
        data: json!({"game":{"state":{"current":16,"interrupt":0},"effect":{
            "effectList":[{"id":3020,"uid":1,"volume":1,"badge":{"id":600050}}],
            "packCandidates":[{"id":3020,"badgeId":600050}],"maxEffectVolume":8,"packDisableSkip":false
        }}}),
    }, 1);
    let initial = services.game_state();
    let check = |expected_selection: f64, expected_sale: f64| {
        let state = services.game_state();
        assert!(state.get("provider_state").is_none());
        assert_eq!(state["session_id"], initial["session_id"]);
        let typed: shanten_backend::ipc::GameStateData = serde_json::from_value(state).unwrap();
        let state = serde_json::to_value(typed).unwrap();
        for (kind, expected) in [("select_candidate", expected_selection), ("sell_effect", expected_sale)] {
            let action = state["action_report"]["actions"].as_array().unwrap().iter()
                .find(|action| action["kind"] == kind).unwrap();
            let score = state["decision_report"]["actions"].as_array().unwrap().iter()
                .find(|score| score["action_id"] == action["id"]).unwrap();
            assert_eq!(score["total"].as_f64(), Some(expected), "{kind}");
        }
    };
    check(-3.0, 20.0);
    let mut book = json!({"version":1,"selected_profile_id":"a","profiles":[
        {"id":"a","name":"A","strategy":"opening","end_count":1,"cutoff_level":13001,"cutoff_node":2,
            "targets":[{"kind":"amulet","id":302,"value":2},{"kind":"badge","id":600050,"value":3}]},
        {"id":"b","name":"B","strategy":"opening","end_count":1,"cutoff_level":13001,"cutoff_node":2,
            "targets":[{"kind":"amulet","id":303}]}
    ]});
    services.patch_config(&json!({"autorun":{"playbook":book}})).unwrap();
    assert!(services.game_state()["revision"].as_u64().unwrap() > initial["revision"].as_u64().unwrap());
    check(-201.0, 20.0);
    book["selected_profile_id"] = json!("b");
    services.patch_config(&json!({"autorun":{"playbook":book}})).unwrap();
    check(-3.0, 20.0);
    book["selected_profile_id"] = json!("a");
    book["profiles"][0]["targets"][0]["value"] = json!(1);
    std::fs::write(root.join("configs/autorun.json"), serde_json::to_vec(&json!({"playbook":book})).unwrap()).unwrap();
    services.reload_files();
    check(-102.0, 20.0);
    services.patch_config(&json!({"autorun":{"playbook":{"version":1,"selected_profile_id":"","profiles":[]}}})).unwrap();
    check(-3.0, 20.0);
    drop(services);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn imp_gain_scores_normal_plus_and_gray_candidates_using_resulting_capacity() {
    for id in [3790,3791,3490,3491,4040,4041] {
        for (used, expected) in [(6,-15.0),(7,0.0)] {
            let state = json!({"provider_state":{"effect":{"maxEffectVolume":8,
                "effectList":[{"id":90,"badgeId":0,"volume":used}]}}});
            let mut actions = candidate("select_candidate", json!(id), Availability::Available);
            actions.actions[0].params["effect"]["badgeId"] = json!(0);
            let report = evaluate(&state, &actions);
            let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain").unwrap();
            assert_eq!(rule.score, Some(expected), "id {id}, used {used}");
            assert_eq!(rule.reason.code, if expected == 0.0 { "imp_capacity_full" } else { "imp_gain" });
            assert_eq!(report.actions[0].total, Some(expected));
        }
    }
}

#[test]
fn imp_gain_counts_seal_volume_and_only_the_net_space_of_a_replacement() {
    for (owned, badge, expected) in [
        (json!([{"id":90,"volume":5,"badgeId":0}]),600160,-15.0),
        (json!([{"id":90,"volume":6,"badgeId":0}]),600160,0.0),
        (json!([{"id":3490,"volume":1,"badgeId":0},{"id":90,"volume":6,"badgeId":0}]),0,-15.0),
        (json!([{"id":3490,"volume":1,"badgeId":0},{"id":90,"volume":7,"badgeId":0}]),0,0.0),
    ] {
        let state = json!({"provider_state":{"game":{"maxEffectVolume":8},"effect":{"effectList":owned}}});
        let mut actions = candidate("select_candidate", json!(3490), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(badge);
        let report = evaluate(&state, &actions);
        assert_eq!(report.actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain").unwrap().score,
            Some(expected), "{state}, badge {badge}");
    }
}

#[test]
fn imp_gain_keeps_missing_capacity_unknown_and_ignores_unrelated_actions() {
    for (owned, maximum) in [
        (Value::Null,json!(8)), (json!([{"id":90}]),json!(8)),
        (json!([{"volume":1,"badgeId":0}]),json!(8)),
        (json!([]),Value::Null), (json!([]),json!(-1)),
        (json!([{"volume":u64::MAX},{"volume":1}]),json!(8)),
    ] {
        let state = json!({"provider_state":{"effect":{"effectList":owned,"maxEffectVolume":maximum}}});
        let mut actions = candidate("select_candidate", json!(3490), Availability::Available);
        actions.actions[0].params["effect"]["badgeId"] = json!(0);
        let report = evaluate(&state, &actions);
        let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain").unwrap();
        assert_eq!(rule.score, None);
        assert_eq!(rule.reason.code, "missing_imp_capacity");
    }
    let state = json!({"provider_state":{"effect":{"effectList":[],"maxEffectVolume":8}}});
    for (kind, id) in [("select_candidate",90),("sell_effect",3490),("skip",4040)] {
        let actions = candidate(kind, json!(id), Availability::Available);
        assert!(evaluate(&state, &actions).actions[0].rules.iter().all(|rule| rule.rule_id != "imp_gain"));
    }
    for availability in [Availability::Unavailable,Availability::Unknown] {
        assert!(evaluate(&state, &candidate("select_candidate", json!(3490), availability)).actions.is_empty());
    }
}

#[test]
fn imp_gain_penalizes_trade_and_gamble_once_without_probability_discount() {
    let events = [
        ("trade",json!({"tradeItems":[{"id":6}],"tradeReward":{"type":2}})),
        ("trade",json!({"tradeItems":[],"tradeReward":{"type":1,"effectId":3791,"badgeId":0}})),
        ("trade",json!({"tradeItems":[{"id":6},{"id":6}],"tradeReward":{"type":1,"effectId":4041,"badgeId":0}})),
        ("gamble",json!({"gambleSelections":[54]})),
        ("gamble",json!({"gambleSelections":[54,15,15,15]})),
        ("gamble",json!({"gambleSelections":[54,113,172]})),
    ];
    for (kind, event) in events {
        for (used, expected) in [(7,-15.0),(8,0.0)] {
            let state = json!({"provider_state":{"nodeEvent":event,"effect":{"maxEffectVolume":8,
                "effectList":[{"id":90,"volume":used,"badgeId":0}]}}});
            let actions = candidate(kind, Value::Null, Availability::Available);
            let report = evaluate(&state, &actions);
            let rules: Vec<_> = report.actions[0].rules.iter().filter(|rule| rule.rule_id == "imp_gain").collect();
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].score, Some(expected), "{kind}, {event}, used {used}");
        }
    }
    let state = json!({"provider_state":{"effect":{"maxEffectVolume":8,"effectList":[{"volume":7}]}}});
    let actions = candidate("buy_effect", json!(3491), Availability::Available);
    assert_eq!(evaluate(&state, &actions).actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain").unwrap().score,
        Some(-15.0));
}

#[test]
fn imp_gain_stacks_with_price_happiness_skip_and_sale_followup_capacity() {
    let mut owned = vec![json!({"id":90,"uid":1,"volume":1,"badgeId":0})];
    owned.extend((2..=7).map(|uid| json!({"id":100,"uid":uid,"volume":1,"badgeId":0})));
    let mut state = json!({"source":"packet","stage":16,"provider_state":{
        "state":{"interrupt":0},"effect":{"effectList":owned,"maxEffectVolume":8,"packDisableSkip":false,
            "packCandidates":[{"id":3490,"badgeId":600110}]}
    }});
    let registry = json!({"amulets":[{"id":349,"sell_price":6},{"id":9,"sell_price":9},{"id":10,"sell_price":0}]});
    for (used, expected) in [(7,26.0),(6,11.0)] {
        state["provider_state"]["effect"]["effectList"].as_array_mut().unwrap().truncate(used);
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        for (kind, expected) in [("select_candidate",expected),("skip",-expected)] {
            let action = actions.actions.iter().find(|action| action.kind == kind).unwrap();
            assert_eq!(report.actions.iter().find(|score| score.action_id == action.id).unwrap().total, Some(expected));
        }
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 1).unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(if used == 7 { -1.0 } else { 14.0 })); // 14 sale + (11 after - 26/11 before).
        let followup = score.rules.iter().find(|rule| rule.rule_id == "sale_followup");
        if used == 7 { assert_eq!(followup.unwrap().score, Some(-15.0)); }
        else { assert!(followup.is_none()); }
    }
}

#[test]
fn imp_gain_from_quiet_home_only_applies_when_entering_battle_with_space() {
    for id in [3010,3011] {
        for (stage, node, used, expected) in [(3,1,7,Some(-15.0)),(3,1,8,Some(0.0)),(9,1,7,None),(3,2,7,None)] {
            let state = json!({"stage":stage,"provider_state":{"map":{"level":13001,"node":node},"effect":{"maxEffectVolume":8,
                "effectList":[{"id":id,"badgeId":0,"volume":used}]}}});
            let actions = candidate("next_node", Value::Null, Availability::Available);
            let report = evaluate(&state, &actions);
            let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain");
            assert_eq!(rule.and_then(|rule| rule.score), expected, "id {id}, stage {stage}, used {used}");
        }
    }
    let state = json!({"stage":3,"provider_state":{"effect":{"maxEffectVolume":8,
        "effectList":[{"id":3010,"badgeId":0,"volume":1}]}}});
    let report = evaluate(&state, &candidate("next_node", Value::Null, Availability::Available));
    let rule = report.actions[0].rules.iter().find(|rule| rule.rule_id == "imp_gain").unwrap();
    assert_eq!(rule.score, None);
    assert_eq!(rule.reason.code, "missing_imp_outcome");
}

#[test]
fn skip_subtracts_the_best_complete_candidate_score_and_keeps_its_other_rules() {
    let state = json!({"provider_state":{"effect":{"effectList":[]}}});
    let book = json!({"selected_profile_id":"opening","profiles":[{"id":"opening","strategy":"opening",
        "targets":[{"kind":"amulet","id":302,"value":2},{"kind":"badge","id":600070,"value":3}]}]});
    let registry = json!({"amulets":[{"id":302,"sell_price":3},{"id":303,"sell_price":6},{"id":304,"sell_price":9999}]});
    let mut actions = candidate("select_candidate", json!(3020), Availability::Available);
    actions.actions[0].params["effect"]["badgeId"] = json!(600070);
    actions.actions[0].cost = Some("5".into());
    let mut second = candidate("select_candidate", json!(3030), Availability::Available).actions.remove(0);
    second.id = "second".into();
    actions.actions.push(second);
    for (id, availability) in [("blocked", Availability::Unavailable), ("unknown", Availability::Unknown)] {
        let mut action = candidate("select_candidate", json!(3040), availability).actions.remove(0);
        action.id = id.into();
        actions.actions.push(action);
    }
    let mut skip = candidate("skip", Value::Null, Availability::Available).actions.remove(0);
    skip.id = "skip".into();
    skip.cost = Some("2".into());
    actions.actions.insert(0, skip);
    let report = decision::evaluate(&state, &actions, &registry, &book);
    assert_eq!(report.actions[1].total, Some(523.0)); // 495 target + 30 guidance + 3 price - 5 cost
    assert_eq!(report.actions[2].total, Some(6.0));
    assert_eq!(report.actions[0].action_id, "skip");
    assert_eq!(report.actions[0].total, Some(-525.0));
    assert_eq!(report.actions[0].rules.iter().map(|rule| (rule.rule_id.as_str(), rule.score)).collect::<Vec<_>>(),
        vec![("coin_cost", Some(-2.0)), ("skip_candidates", Some(-523.0))]);
}

#[test]
fn skip_uses_the_actual_maximum_even_when_all_candidates_are_negative_or_zero() {
    let registry = json!({"amulets":[{"id":302,"sell_price":3}]});
    for (costs, expected) in [([8,5], 2.0), ([3,3], 0.0), ([0,1], -3.0)] {
        let mut actions = candidate("skip", Value::Null, Availability::Available);
        actions.actions[0].id = "skip".into();
        for (index, cost) in costs.into_iter().enumerate() {
            let mut action = candidate("select_candidate", json!(3020), Availability::Available).actions.remove(0);
            action.id = format!("candidate-{index}");
            action.cost = Some(cost.to_string());
            actions.actions.push(action);
        }
        let report = evaluate_with_registry(&json!({}), &actions, &registry);
        assert_eq!(report.actions[0].total, Some(expected));
        assert_eq!(report.actions[0].rules[0].reason.code, "skip_candidates");
    }
}

#[test]
fn skip_rule_requires_available_skip_and_selection_actions_and_preserves_score_limits() {
    let registry = json!({"amulets":[{"id":302,"sell_price":3}]});
    for kind in ["buy_effect", "buy_pack", "remake"] {
        let mut actions = candidate("skip", Value::Null, Availability::Available);
        let mut other = candidate(kind, json!(3020), Availability::Available).actions.remove(0);
        other.id = "other".into();
        actions.actions.push(other);
        assert!(evaluate_with_registry(&json!({}), &actions, &registry).actions[0].rules.is_empty());
    }
    let mut actions = candidate("select_candidate", json!(3020), Availability::Available);
    let mut skip = candidate("skip", Value::Null, Availability::Available).actions.remove(0);
    skip.id = "skip".into();
    actions.actions.push(skip);
    for availability in [Availability::Unavailable, Availability::Unknown] {
        actions.actions[0].availability = availability.clone();
        let report = evaluate_with_registry(&json!({}), &actions, &registry);
        assert_eq!(report.actions.len(), 1);
        assert!(report.actions[0].rules.is_empty());
        actions.actions[0].availability = Availability::Available;
        actions.actions[1].availability = availability;
        let report = evaluate_with_registry(&json!({}), &actions, &registry);
        assert_eq!(report.actions.len(), 1);
        assert_eq!(report.actions[0].action_id, "candidate-0");
        actions.actions[1].availability = Availability::Available;
    }
    actions.actions[1].cost = Some("9007199254740991".into());
    assert_eq!(evaluate_with_registry(&json!({}), &actions, &registry).actions[1].total, None);
}

#[test]
fn published_scores_follow_the_current_snapshot_and_survive_ipc() {
    use shanten_backend::{pipeline::{Direction, Packet}, services::Services};
    let root = std::env::temp_dir().join(format!("lens-decision-{}", std::process::id()));
    let (events, _) = tokio::sync::broadcast::channel(16);
    let services = Services::load(root.join("configs"), events).unwrap();
    let mut packet = Packet {
        direction: Direction::Inbound, packet_type: "Res".into(), id: Some(1),
        method: ".lq.Lobby.fetchAmuletActivityData".into(),
        data: json!({"game":{"state":{"current":16},"effect":{
            "effectList":[],"packCandidates":[{"id":2240,"badgeId":0}],"maxEffectVolume":8,"packDisableSkip":false
        }}}),
    };
    for (stage, interrupt, owned, badge, expected) in [
        (16, 0, json!([]), 0, 59.0),
        (9, 1000, json!([{"id":2241,"uid":1,"volume":1}]), 0, 0.0),
        (2, 0, json!([]), 0, 59.0),
        (16, 0, json!([]), 600050, 68.0),
        (16, 0, json!([]), 600110, 79.0),
        (16, 0, json!([
            {"id":3020,"uid":1,"volume":1,"badge":{"id":600070}},
            {"id":3030,"uid":2,"volume":1,"badge":{"id":600070}},
            {"id":3040,"uid":3,"volume":1,"badge":{"id":600070}}
        ]), 600070, 79.0),
    ] {
        packet.data["game"]["state"] = json!({"current":stage,"interrupt":interrupt});
        packet.data["game"]["effect"]["effectList"] = owned;
        packet.data["game"]["effect"]["packCandidates"][0]["badgeId"] = json!(badge);
        services.update_game_state_from_flow(&packet, 1);
        let state = services.game_state();
        assert!(state.get("provider_state").is_none());
        let typed: shanten_backend::ipc::GameStateData = serde_json::from_value(state).unwrap();
        let state = serde_json::to_value(typed).unwrap();
        let action = state["action_report"]["actions"].as_array().unwrap().iter()
            .find(|action| action["kind"] == "select_candidate").unwrap();
        let score = state["decision_report"]["actions"].as_array().unwrap().iter()
            .find(|score| score["action_id"] == action["id"]).unwrap();
        assert_eq!(score["total"].as_f64(), Some(expected));
        let skip = state["action_report"]["actions"].as_array().unwrap().iter()
            .find(|action| action["kind"] == "skip").unwrap();
        let skip_score = state["decision_report"]["actions"].as_array().unwrap().iter()
            .find(|score| score["action_id"] == skip["id"]);
        if stage == 2 { assert!(skip_score.is_none()); }
        else { assert_eq!(skip_score.unwrap()["total"].as_f64(), Some(-expected)); }
    }
    packet.data = json!({"data":{"activityId":261081,"game":{
        "state":{"current":15,"interrupt":0}, "game":{"coin":"50"},
        "character":{"hp":50,"maxHp":100}, "effect":{"effectList":[]},
        "nodeEvent":{"gambleId":2,"gamblePrice":5,"gambleSelections":[6,26,13,11,11,42]}
    }}});
    services.update_game_state_from_flow(&packet, 1);
    let typed: shanten_backend::ipc::GameStateData = serde_json::from_value(services.game_state()).unwrap();
    let state = serde_json::to_value(typed).unwrap();
    let action = state["action_report"]["actions"].as_array().unwrap().iter()
        .find(|action| action["kind"] == "gamble").unwrap();
    let score = state["decision_report"]["actions"].as_array().unwrap().iter()
        .find(|score| score["action_id"] == action["id"]).unwrap();
    assert!((score["total"].as_f64().unwrap() - 65.0 / 3.0).abs() < 1e-10);
    for (owned, expected) in [
        (json!([]), [4.4898, 6.2925, 14.1773]),
        (json!([{"id":3020,"uid":1,"volume":1,"badge":{"id":600070}}]),
            [4.985938056247775, 6.762396694214876, 14.383920847750865]),
    ] {
        packet.data = json!({"game":{
            "state":{"current":9,"interrupt":0},"game":{"coin":"50"},
            "effect":{"effectList":owned,"shopBuffList":[]},
            "shop":{"goods":[
                {"id":1,"goodsId":101,"price":5,"sold":false},
                {"id":2,"goodsId":102,"price":10,"sold":false},
                {"id":3,"goodsId":103,"price":15,"sold":false}
            ]}
        }});
        services.update_game_state_from_flow(&packet, 1);
        let typed: shanten_backend::ipc::GameStateData = serde_json::from_value(services.game_state()).unwrap();
        let state = serde_json::to_value(typed).unwrap();
        let actions: Vec<_> = state["action_report"]["actions"].as_array().unwrap().iter()
            .filter(|action| action["kind"] == "buy_pack").collect();
        assert_eq!(actions.len(), 3);
        for (index, action) in actions.iter().enumerate() {
            let score = state["decision_report"]["actions"].as_array().unwrap().iter()
                .find(|score| score["action_id"] == action["id"]).unwrap();
            let value = score["rules"].as_array().unwrap().iter().find(|rule| rule["rule_id"] == "pack_value").unwrap();
            assert!((value["score"].as_f64().unwrap() - (expected[index] - if index == 2 { 5.0 } else { 0.0 })).abs() < 1e-12);
            let expected_total = if index == 0 { 1.0 } else { expected[index] - 5.0 * (index + 1) as f64 };
            assert!((score["total"].as_f64().unwrap() - expected_total).abs() < 1e-12);
        }
    }
    drop(services);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sale_followup_only_rewards_new_choice_value_in_the_colorful_knight_screenshot() {
    let registry = json!({"amulets":[
        {"id":356,"sell_price":9,"plus_sell_price":9},
        {"id":339,"sell_price":6},{"id":321,"sell_price":3},{"id":9,"sell_price":0}
    ]});
    for (full, expected, followup) in [(false,-6.0,None),(true,10.0,Some(6.0))] {
        let mut owned = vec![json!({"id":3561,"uid":1,"volume":1,"badgeId":0})];
        if full { owned.extend((2..=8).map(|uid| json!({"id":90,"uid":uid,"volume":1,"badgeId":0}))); }
        let state = json!({"source":"packet","stage":16,"provider_state":{
            "state":{"interrupt":0},"effect":{"effectList":owned,"maxEffectVolume":8,"packDisableSkip":false,
                "packCandidates":[{"id":3390,"badgeId":0},{"id":3210,"badgeId":0}]}
        }});
        let actions = shanten_backend::action_provider::evaluate(&state);
        let doctor = actions.actions.iter().find(|action| action.kind == "select_candidate" && action.params["effect"]["id"] == 3390).unwrap();
        assert_eq!(doctor.availability, if full { Availability::Unavailable } else { Availability::Available });
        let report = evaluate_with_registry(&state, &actions, &registry);
        if !full { assert_eq!(report.actions.iter().find(|score| score.action_id == doctor.id).unwrap().total, Some(6.0)); }
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 1).unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(expected)); // 9 + 5 - 10, then -10 without improvement or +6 for unlocking the choice.
        let rule = score.rules.iter().find(|rule| rule.rule_id == "sale_followup");
        assert_eq!(rule.and_then(|rule| rule.score), followup);
        if !full { assert!(rule.is_none()); }
        let spare = score.rules.iter().find(|rule| rule.rule_id == "retain_amulet_spare_capacity");
        assert_eq!(spare.and_then(|rule| rule.score), if full { None } else { Some(-10.0) });
    }
}

#[test]
fn retained_sales_without_choice_improvement_are_penalized_regardless_of_capacity() {
    let registry = json!({"amulets":[{"id":356,"plus_sell_price":9}]});
    for (owned, maximum) in [
        (json!([{"id":3561,"uid":1,"badgeId":600160}]),json!(3)),
        (json!([{"id":3561,"uid":1,"badgeId":600160}]),json!(2)),
        (json!([{"id":3561,"uid":1,"badgeId":600160,"volume":1}]),json!(2)),
        (json!([{"id":3561,"uid":1,"badgeId":0},{"id":90}]),json!(8)),
        (Value::Null,json!(8)),
        (json!([{"id":3561,"uid":1,"badgeId":0}]),Value::Null),
    ] {
        for fallback in [false,true] {
            let mut state = json!({"source":"packet","stage":9,"provider_state":{
                "game":{"maxEffectVolume":maximum},"effect":{"effectList":owned}
            }});
            if !fallback { state["provider_state"]["effect"]["maxEffectVolume"] = maximum.clone(); }
            let mut actions = candidate("sell_effect", json!(3561), Availability::Available);
            actions.actions[0].params["effect"]["badgeId"] = json!(0);
            let report = evaluate_with_registry(&state, &actions, &registry);
            let score = &report.actions[0];
            assert_eq!(score.total, Some(-6.0), "{state}");
            let rule = score.rules.iter().find(|rule| rule.rule_id == "retain_amulet_spare_capacity");
            assert_eq!(rule.and_then(|rule| rule.score), Some(-10.0));
            if let Some(rule) = rule { assert_eq!(rule.reason.code, "retain_amulet_spare_capacity"); }
            assert!(score.rules.iter().all(|rule| rule.rule_id != "sale_followup"));
        }
    }
}

#[test]
fn retained_sales_only_add_the_extra_penalty_when_followup_does_not_improve() {
    let registry = json!({"amulets":[{"id":356,"sell_price":9,"plus_sell_price":9},
        {"id":339,"sell_price":6},{"id":349,"sell_price":6},{"id":9,"sell_price":0}]});
    for (filler_volume, choice, followup, penalty, expected) in [
        (0,json!({"id":3560,"badgeId":600050}),Some(9.0),false,13.0),
        (6,json!({"id":3390,"badgeId":600160}),Some(6.0),false,10.0),
        (6,json!({"id":3490,"badgeId":600110}),Some(-15.0),true,-21.0),
        (0,json!({"id":3210,"badgeId":0}),None,false,4.0),
    ] {
        let mut owned = vec![json!({"id":3561,"uid":1,"volume":1,"badgeId":0})];
        if filler_volume > 0 { owned.push(json!({"id":90,"uid":2,"volume":filler_volume,"badgeId":0})); }
        let state = json!({"source":"packet","stage":16,"provider_state":{
            "state":{"interrupt":0},"effect":{"effectList":owned,"maxEffectVolume":8,"packDisableSkip":false,
                "packCandidates":[choice]}
        }});
        let actions = shanten_backend::action_provider::evaluate(&state);
        let report = evaluate_with_registry(&state, &actions, &registry);
        let sale = actions.actions.iter().find(|action| action.kind == "sell_effect" && action.params["uid"] == 1).unwrap();
        let score = report.actions.iter().find(|score| score.action_id == sale.id).unwrap();
        assert_eq!(score.total, Some(expected), "{state}");
        assert_eq!(score.rules.iter().find(|rule| rule.rule_id == "sale_followup").unwrap().score, followup);
        let rule = score.rules.iter().find(|rule| rule.rule_id == "retain_amulet_spare_capacity");
        assert_eq!(rule.and_then(|rule| rule.score), if penalty { Some(-10.0) } else { None });
    }
}
