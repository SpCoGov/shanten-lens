import assert from "node:assert/strict";
import {activeBuyPackHotkeys, selectAmuletPack} from "../src/lib/amuletPackHotkeys";
import type {ActionCandidate, ActionReport} from "../src/bindings";
import type {GoodsItem} from "../src/lib/gamestate";

const goods: GoodsItem[] = [
    {id: 20, goodsId: 103, price: 9, sold: false},
    {id: 40, goodsId: 101, price: 3, sold: false},
    {id: 60, goodsId: 102, price: 6, sold: false},
    {id: 80, goodsId: 113, price: 9, sold: false},
];
assert.equal(selectAmuletPack(goods, "9", "low_to_high"), goods[1]);
assert.equal(selectAmuletPack(goods, "9", "high_to_low"), goods[0]);
assert.equal(selectAmuletPack(goods, "9", "legendary_only"), goods[0]);
assert.equal(selectAmuletPack(goods, "6", "high_to_low"), goods[2]);
assert.equal(selectAmuletPack(goods, "6", "legendary_only"), undefined);
assert.equal(selectAmuletPack(goods, "2", "low_to_high"), undefined);
assert.equal(selectAmuletPack([], "99", "high_to_low"), undefined);
assert.equal(selectAmuletPack(goods.map((good) => ({...good, sold: true})), "99", "low_to_high"), undefined);
assert.equal(selectAmuletPack(goods.map((good, i) => ({...good, sold: i === 0})), "9", "legendary_only")?.id, 80);
assert.equal(selectAmuletPack(goods.slice(1, 3), "99", "legendary_only"), undefined);
assert.equal(selectAmuletPack([{...goods[0], goodsId: 203}], "99", "legendary_only"), undefined);
assert.equal(selectAmuletPack([{...goods[0], price: "invalid"}], "99", "legendary_only"), undefined);
assert.equal(selectAmuletPack([{...goods[0], price: -1}], "99", "legendary_only"), undefined);
assert.equal(selectAmuletPack(goods, "invalid", "low_to_high"), undefined);
assert.equal(selectAmuletPack([{...goods[0], price: "9007199254740993"}], "9007199254740992", "legendary_only"), undefined);
assert.equal(selectAmuletPack([{...goods[0], price: 0}], "0", "legendary_only")?.id, 20);

const action = (id: number, availability: ActionCandidate["availability"]): ActionCandidate => ({
    id: `buy_pack:${id}`, kind: "buy_pack", params: {id}, cost: "9", cost_resource: "hp", availability, reasons: [],
});
const report: ActionReport = {
    stage: 9, interrupt: null, status: "ready",
    actions: [action(20, "unavailable"), action(40, "unknown"), action(60, "available"), action(80, "available")],
    global_actions: [], issues: [],
};
assert.equal(selectAmuletPack(goods, "99", "low_to_high", report), goods[2]);
assert.equal(selectAmuletPack(goods, "99", "legendary_only", report), goods[3]);
// An available HP purchase must not be rejected just because the player has no coins.
assert.equal(selectAmuletPack(goods, "0", "high_to_low", report), goods[3]);
assert.equal(selectAmuletPack(goods, "99", "legendary_only", {...report, actions: [action(20, "unknown")]}), undefined);
assert.equal(selectAmuletPack(goods, "99", "low_to_high", {...report, actions: []}), undefined);
assert.equal(selectAmuletPack(goods, "99", "low_to_high", {...report, status: "waiting_state"}), undefined);
assert.equal(selectAmuletPack(goods, "99", "low_to_high", {...report, stage: 2}), undefined);
// Native Qingyun IPC has no action report yet; its shop prices still drive selection.
assert.equal(selectAmuletPack(goods, "9", "legendary_only", {...report, status: "unsupported_source", actions: []}), goods[0]);
assert.equal(selectAmuletPack(goods, "6", "legendary_only", null), undefined);
assert.deepEqual(goods.map((good) => good.id), [20, 40, 60, 80]);

const buyPack = ["q", "w", "e", "r", "t"];
assert.deepEqual(activeBuyPackHotkeys({buyPackMode: "single", buyPack}), ["q"]);
assert.deepEqual(activeBuyPackHotkeys({buyPackMode: "multiple", buyPack}), buyPack);
assert.deepEqual(buyPack, ["q", "w", "e", "r", "t"]);
console.log("Amulet pack hotkey assertions passed");
