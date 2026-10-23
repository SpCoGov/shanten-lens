import assert from "node:assert/strict";
import {
    readWanxiangPreferences, updateWanxiangPreferences, WANXIANG_COIN_OBJECTIVE_KEY,
    WANXIANG_PREFER_BONUS_KEY, WANXIANG_PREFERRED_SUIT_KEY, WANXIANG_PREFERRED_MELD_TYPE_KEY,
} from "./wanxiangSettings";
import type {SearchPreferences} from "./planTypes";

const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
const saved = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {configurable: true, value: {
    getItem: (key: string) => saved.get(key) ?? null,
}});
try {
    const legacy: SearchPreferences = {prefer_dora: true, prefer_soul: true, any_waits: true,
        preferred_suit: "p", preferred_meld_type: "triplet"};
    for (const coin_objective of ["heavenly_blessing", "shining_pearl"] as const) {
        const exclusive = updateWanxiangPreferences(legacy, {coin_objective});
        assert.deepEqual(exclusive, {coin_objective, prefer_dora: false, prefer_soul: false,
            any_waits: false, preferred_suit: null, preferred_meld_type: null});
        assert.equal(updateWanxiangPreferences(exclusive, {preferred_suit: "s"}).coin_objective, null);
        assert.equal(updateWanxiangPreferences(exclusive, {preferred_meld_type: "sequence"}).coin_objective, null);
        assert.equal(updateWanxiangPreferences(exclusive, {prefer_dora: true, prefer_soul: true}).coin_objective, null);
        assert.equal(updateWanxiangPreferences(exclusive, {coin_objective: null}).coin_objective, null);
        const other = coin_objective === "heavenly_blessing" ? "shining_pearl" : "heavenly_blessing";
        assert.equal(updateWanxiangPreferences(exclusive, {coin_objective: other}).coin_objective, other);
        saved.set(WANXIANG_PREFER_BONUS_KEY, "1");
        saved.set(WANXIANG_PREFERRED_SUIT_KEY, "p");
        saved.set(WANXIANG_PREFERRED_MELD_TYPE_KEY, "triplet");
        saved.set(WANXIANG_COIN_OBJECTIVE_KEY, coin_objective);
        assert.deepEqual(readWanxiangPreferences(), exclusive);
    }
    saved.set(WANXIANG_COIN_OBJECTIVE_KEY, "invalid");
    assert.deepEqual(readWanxiangPreferences(), {...legacy, any_waits: false, coin_objective: null});
    saved.clear();
    assert.deepEqual(readWanxiangPreferences(), {prefer_dora: false, prefer_soul: false, any_waits: false,
        preferred_suit: null, preferred_meld_type: null, coin_objective: null});
} finally {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else Reflect.deleteProperty(globalThis, "localStorage");
}
