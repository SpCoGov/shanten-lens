import assert from "node:assert/strict";
import fixture from "../../../backend/tests/fixtures/playbook.json";
import {moveItem, handFromTargetPreset, removeStrategyAmulet, validateAutoPlayConfig, openingLevels, strategyLevels, getOpeningLevels, getStrategyLevels, newTacticalStage, type FullPlayProfile, type OpeningPlayProfile} from "./autoPlayConfig";
import {getGameLevels, getLevelKey} from "./gameStages";
import {formatLevelIdToLabel} from "./levelFormat";
import {compactTargetPreset, readTargetPresets, targetSize, TARGET_PRESETS_KEY, type TargetPreset} from "./customTarget";
import type {Amulet} from "./registryStore";
import {getAutoRunnerSnapshot, markPlaybookSaved, patchAutoConfig, setAutoConfig} from "./autoRunnerStore";

const valid = fixture as {version: 1; selected_profile_id: string; profiles: [FullPlayProfile, OpeningPlayProfile]};
assert.equal(validateAutoPlayConfig(valid), null);
const original = valid.profiles[0].stages[0].loadouts[0];
assert.deepEqual(moveItem(original.entry_order, 0, 1), ["b", "a"]);
assert.deepEqual(original.entry_order, ["a", "b"]);
const removed = removeStrategyAmulet(original, "a");
assert.deepEqual(removed.entry_order, ["b"]);
assert.deepEqual(removed.win_order, ["b"]);
assert.equal(removed.amulets[0].key, "b");
assert.equal(original.amulets.length, 2);
assert.deepEqual(moveItem(["a", "b", "c", "d"], 0, 3), ["b", "c", "d", "a"]);
assert.deepEqual(moveItem(["a", "b", "c", "d"], 3, -2), ["a", "d", "b", "c"]);
assert.deepEqual(moveItem(["a", "b"], 0, -1), ["a", "b"]);

const preset: TargetPreset = {format: "shanten-lens.custom-target", version: 1, name: "Seven pairs",
    target_groups: valid.profiles[0].stages[0].hands[0].target_groups, wall_limit: 24, exclude_invalid_shapes: true};
const storage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
Object.defineProperty(globalThis, "localStorage", {configurable: true, value: {getItem: (key: string) =>
    key === TARGET_PRESETS_KEY ? JSON.stringify([compactTargetPreset(preset), {format: "broken"}]) : null}});
try {
    const saved = readTargetPresets();
    assert.equal(saved.length, 1);
    const hand = handFromTargetPreset(saved[0]);
    assert.equal(hand.name, preset.name);
    assert.equal(hand.exclude_invalid_shapes, preset.exclude_invalid_shapes);
    assert.equal(targetSize(hand.target_groups), 14);
    assert.deepEqual(hand.target_groups, saved[0].target_groups);
    hand.target_groups[0].rule.faces.push("3m");
    assert.notDeepEqual(hand.target_groups, saved[0].target_groups);
    assert.equal("wall_limit" in hand, false);
} finally {
    if (storage) Object.defineProperty(globalThis, "localStorage", storage);
    else Reflect.deleteProperty(globalThis, "localStorage");
}

for (const mutate of [
    (v: typeof valid) => {v.selected_profile_id = "missing";},
    (v: typeof valid) => {v.profiles[0].first_level_min_coin = "01";},
    (v: typeof valid) => {v.profiles[0].stages[0].deadline_level = 999;},
    (v: typeof valid) => {v.profiles[0].stages[0].deadline_node = 99;},
    (v: typeof valid) => {v.profiles[0].stages[0].start_node = 1;},
    (v: typeof valid) => {v.profiles[0].stages[1].start_level = 102;},
    (v: typeof valid) => {v.profiles[0].stages[0].loadouts[0].win_order = ["a", "a"];},
    (v: typeof valid) => {v.profiles[0].stages[0].hands[0].target_groups[0].rule.faces = ["10m"];},
    (v: typeof valid) => {v.profiles[0].stages[0].hands[0].target_groups[0].kind = "single";},
    (v: typeof valid) => {v.profiles[1].end_count = 0;},
    (v: typeof valid) => {v.profiles[1].end_count = Number.MAX_SAFE_INTEGER + 1;},
    (v: typeof valid) => {v.profiles[1].targets = [];},
    (v: typeof valid) => {v.profiles[1].targets[0].value = 1.5;},
    (v: typeof valid) => {v.profiles[1].cutoff_level = 999;},
    (v: typeof valid) => {v.profiles[1].cutoff_level = 203;},
    (v: typeof valid) => {v.profiles[1].cutoff_node = 1;},
    (v: typeof valid) => {v.profiles[1].cutoff_node = 99;},
    (v: typeof valid) => {Reflect.deleteProperty(v.profiles[1], "cutoff_node");},
    (v: typeof valid) => {Reflect.deleteProperty(v.profiles[1], "cutoff_level");},
]) {
    const invalid = structuredClone(valid);
    mutate(invalid);
    assert.notEqual(validateAutoPlayConfig(invalid), null);
}
const registry = new Map<number, Amulet>([[1, {id: 1, icon_id: 1, name: "test", rarity: "ORANGE"}]]);
assert.equal(validateAutoPlayConfig(valid, registry), "initial");
registry.get(1)!.rarity = "GREEN";
assert.equal(validateAutoPlayConfig(valid, registry), null);
assert.equal(validateAutoPlayConfig({...valid, selected_profile_id: valid.profiles[1].id, profiles: [valid.profiles[1]]}, registry, new Map()), "targets");
for (const strategy of ["unknown", null, 0]) {
    const invalid = structuredClone(fixture);
    Object.assign(invalid.profiles[0], {strategy});
    assert.equal(validateAutoPlayConfig(invalid), "strategy");
}
const explicit = structuredClone(valid);
explicit.profiles[0].strategy = "full";
explicit.profiles[1].cutoff_level = 0;
explicit.profiles[1].cutoff_node = 0;
explicit.profiles[1].targets = [{kind: "amulet", id: 1}, {kind: "badge", id: 600010}];
assert.equal(validateAutoPlayConfig(explicit), "cutoff", "Opening mode requires a cutoff even when its targets are valid.");
for (const [level, node, label] of [[13001, 2, "1-1"], [13001, 3, "1-2"], [13001, 6, "1-BOSS"], [14001, 3, "Ex1-BOSS"]] as const) {
    explicit.profiles[1].cutoff_level = level;
    explicit.profiles[1].cutoff_node = node;
    assert.equal(validateAutoPlayConfig(explicit), null);
    assert.equal(formatLevelIdToLabel(level, "packet", node), label);
}
assert.equal(openingLevels.length, getGameLevels().reduce((count, level) => count + level.nodes.filter(node => node.type !== 1).length, 0));
assert.equal(new Set(openingLevels.map(getLevelKey)).size, openingLevels.length);
const stage = newTacticalStage("stage", "loadout", "hand");
assert.deepEqual([stage.start_level, stage.start_node, stage.deadline_level, stage.deadline_node],
    [strategyLevels[0].level, strategyLevels[0].node, strategyLevels[0].level, strategyLevels[0].node]);
explicit.profiles[0].stages[1].start_level = 13001;
explicit.profiles[0].stages[1].start_node = 4;
assert.equal(validateAutoPlayConfig(explicit), null, "Later nodes on the same map are separate stages.");
explicit.profiles[0].stages[1].start_node = 3;
assert.equal(validateAutoPlayConfig(explicit), "levels", "Duplicate stage boundaries are rejected.");
assert.deepEqual(JSON.parse(JSON.stringify(valid)).profiles[1], valid.profiles[1]);
assert.equal(JSON.parse(JSON.stringify(valid)).profiles[0].first_level_min_coin, "9007199254740993");

setAutoConfig({op_interval_ms: 1000, playbook: valid});
const draft = structuredClone(valid);
draft.profiles[0].name = "Unsaved draft";
patchAutoConfig({playbook: draft});
setAutoConfig({op_interval_ms: 700, playbook: valid});
assert.equal(getAutoRunnerSnapshot().config.op_interval_ms, 700);
assert.equal(getAutoRunnerSnapshot().config.playbook, draft);
assert.equal(getAutoRunnerSnapshot().playbookDirty, true);
markPlaybookSaved(valid);
assert.equal(getAutoRunnerSnapshot().playbookDirty, true);
markPlaybookSaved(draft);
assert.equal(getAutoRunnerSnapshot().playbookDirty, false);
setAutoConfig({op_interval_ms: 700, playbook: valid});
assert.equal(getAutoRunnerSnapshot().config.playbook, valid);
assert.equal(getOpeningLevels("packet"), openingLevels);
assert.equal(getStrategyLevels("packet"), strategyLevels);
assert.equal(getOpeningLevels("qyzz").length, 314);
assert.deepEqual(getOpeningLevels("qyzz")[0], {level: 101, node: 0, label: "1-1"});
assert.deepEqual(getStrategyLevels("qyzz")[0], {level: 102, node: 0, label: "1-2"});
assert.deepEqual(getOpeningLevels("qyzz").slice(-1)[0], {level: 1299, node: 0, label: "Ex299"});
const native = structuredClone(valid);
Object.assign(native.profiles[0].stages[0], {start_level: 102, start_node: 0, deadline_level: 103, deadline_node: 0});
Object.assign(native.profiles[0].stages[1], {start_level: 201, start_node: 0, deadline_level: 1299, deadline_node: 0});
Object.assign(native.profiles[1], {cutoff_level: 1299, cutoff_node: 0});
assert.equal(validateAutoPlayConfig(native), null);
native.profiles[1].cutoff_level = 1300;
assert.equal(validateAutoPlayConfig(native), "cutoff");
native.profiles[1].cutoff_level = 1299;
native.profiles[0].stages[1].start_level = 13001;
native.profiles[0].stages[1].start_node = 3;
assert.equal(validateAutoPlayConfig(native), "levels", "A tactical profile cannot mix native and packet level references.");
console.log("Autoplay configuration checks passed.");


const badgeCountBook = structuredClone(valid);
for (const count of [1, 2, Number.MAX_SAFE_INTEGER]) {
    badgeCountBook.profiles[1].targets = [{kind: "badge", id: 600010, count}];
    assert.equal(validateAutoPlayConfig(badgeCountBook), null);
    const saved = JSON.parse(JSON.stringify(badgeCountBook));
    assert.equal(saved.profiles[1].targets[0].count, count);
}
for (const count of [0, -1, 1.5, "2", null, NaN, Number.MAX_SAFE_INTEGER + 1]) {
    Object.assign(badgeCountBook.profiles[1].targets[0], {count});
    assert.equal(validateAutoPlayConfig(badgeCountBook), "targets");
}
badgeCountBook.profiles[1].targets = [{kind: "badge", id: 600010}];
assert.equal(validateAutoPlayConfig(badgeCountBook), null, "Older badge targets keep the default count of one.");
console.log("Badge target count validation checks passed.");

const characterBook = structuredClone(valid);
for (const profile of characterBook.profiles) {
    for (const id of [201,202,203,204]) {
        profile.character_id = id;
        assert.equal(validateAutoPlayConfig(characterBook),null);
        assert.equal(JSON.parse(JSON.stringify(characterBook)).profiles.find((item: {id: string}) => item.id === profile.id).character_id,id);
    }
}
delete characterBook.profiles[1].character_id;
assert.equal(validateAutoPlayConfig(characterBook),null,"Old opening profiles retain the default role.");
for (const id of [0,200,205,999,"201",null,1.5]) {
    Object.assign(characterBook.profiles[1],{character_id:id});
    assert.equal(validateAutoPlayConfig(characterBook),"profile");
}
console.log("Initial character configuration checks passed.");

const vineBook = {version: 1, selected_profile_id: "vine", profiles: [{id: "vine", name: "Vine", strategy: "vine_farming"}]};
assert.equal(validateAutoPlayConfig(vineBook), null);
assert.deepEqual(JSON.parse(JSON.stringify(vineBook)), vineBook);
assert.equal(validateAutoPlayConfig({...vineBook, profiles: [{...vineBook.profiles[0], strategy: "unknown"}]}), "strategy");
console.log("Vine farming configuration checks passed.");

for (const reward of [null,62,93,140,210,315,473,710,1065]) {
    const next={...vineBook,profiles:[{...vineBook.profiles[0],reward_refresh_limit:reward,normal_refresh_limit:210,stop_ex:70,pause_half_hp:false}]};
    assert.equal(validateAutoPlayConfig(JSON.parse(JSON.stringify(next))),null);
}
for (const patch of [{reward_refresh_limit:0},{reward_refresh_limit:1},{normal_refresh_limit:1598},{normal_refresh_limit:"210"},
    {stop_ex:0},{stop_ex:71},{stop_ex:1.5},{pause_half_hp:null}]) {
    assert.equal(validateAutoPlayConfig({...vineBook,profiles:[{...vineBook.profiles[0],...patch}]}),"vine");
}
console.log("Vine refresh interval, cutoff and health options round-trip checks passed.");
