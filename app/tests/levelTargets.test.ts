import assert from "node:assert/strict";
import qyzz from "../src/lib/qyzzStageCatalog.json";
import {formatLevelIdToLabel, parseLevelLabelToId} from "../src/lib/levelFormat";
import {getGameStage, getLevelKey} from "../src/lib/gameStages";
import {getFutureLevelTargets, getOrderedLevelTargets} from "../src/lib/levelTargets";
import {calculateCurrentPoint, cloneResolvedRules, parseTargetPointValue, projectFuturePoints, resolveAmuletRule} from "../src/lib/scoreEngine";

assert.equal(formatLevelIdToLabel(13001, "packet", 2), "1-1");
assert.equal(formatLevelIdToLabel(13001, "packet", 3), "1-2");
assert.equal(formatLevelIdToLabel(13001, "packet", 6), "1-BOSS");
assert.equal(formatLevelIdToLabel(13001, "packet"), "1");
assert.equal(formatLevelIdToLabel(14001, "packet", 3), "Ex1-BOSS");
assert.equal(parseLevelLabelToId("1-BOSS", "packet"), 13001);
assert.equal(parseLevelLabelToId("1-BOSS", "packet", 11001), 11001);
assert.notEqual(getLevelKey({level: 13001, node: 2}), getLevelKey({level: 13001, node: 4}));

const mapNodes = [{}, {subType: 3004}, {}, {subType: 3003}, {}, {subType: 3011}];
const targets = getOrderedLevelTargets("packet", 13001, mapNodes);
assert.deepEqual(targets.slice(0, 3).map(stage => [stage.node, stage.label, stage.target]), [[2, "1-1", "100"], [4, "1-3", "800"], [6, "1-BOSS", "750"]]);
assert.ok(targets.every(stage => getGameStage(stage.level, stage.node)?.hp_rate != null));
assert.deepEqual(getOrderedLevelTargets("packet", 13001)[0].target, ["200", "400", "100"], "Keep every possible enemy target until the enemy is known.");
assert.equal(getOrderedLevelTargets("packet", 13001, [{}, {}, {}, {}, {}, {subType: 0}])[2].target, "750", "An unrolled enemy can still have a target when every possible enemy has the same HP.");
const future = getFutureLevelTargets("packet", 13001, 3, mapNodes);
assert.deepEqual(future.slice(0, 3).map(stage => stage.label), ["1-3", "1-BOSS", "2-2"]);
assert.equal(getFutureLevelTargets("packet", 13001, 6)[0].label, "2-2");
assert.equal(getFutureLevelTargets("packet", 999999, 1).length, 0);
assert.ok(getFutureLevelTargets("packet", 11001, 2).every(stage => stage.level < 13000));
const hugeTarget = getOrderedLevelTargets("packet", 14199, [{}, {subType: 3019}]).find(stage => stage.level === 14199)?.target;
assert.ok(typeof hugeTarget === "string" && (parseTargetPointValue(hugeTarget) ?? 0n) > 10n ** 300n);

assert.deepEqual(getOrderedLevelTargets("qyzz"), qyzz.levels.map(stage => ({label: stage.name, level: stage.id, target: stage.target_points})));
assert.equal(formatLevelIdToLabel(1001, "qyzz", 3), "Ex1");
assert.equal(parseLevelLabelToId("Ex1", "qyzz"), 1001);
assert.equal(getFutureLevelTargets("qyzz", 101, 99)[0].level, 102);

const rules = [resolveAmuletRule({id: 999999, uid: 1, volume: 1, tags: [], store: ["0"]}, {
    activeOnWin: true, effectTarget: "none", growthAfterRound: false, growthFormula: "data + 1",
})];
const seed = calculateCurrentPoint(100n, 100n, 13001, cloneResolvedRules(rules));
const projections = projectFuturePoints(13001, future.slice(0, 2), seed, cloneResolvedRules(rules), 100n, 100n, 1, {}, false);
assert.deepEqual(projections.map(stage => stage.node), [4, 6]);
assert.deepEqual(projections.map(stage => stage.amulets[0].dataRaw), ["0", "100"], "A noncombat current node must not add a battle's growth.");
assert.deepEqual(projections.map(getLevelKey), ["13001:4", "13001:6"]);
assert.equal(projectFuturePoints(13001, [future[0]], seed, cloneResolvedRules(rules), 100n, 100n, 1)[0].amulets[0].dataRaw, "100");

for (const [point, reached] of [[99n, false], [100n, null], [200n, null], [399n, null], [400n, true]] as const) {
    const projection = projectFuturePoints(13001, [{level: 13001, node: 2, target: ["400", "100", "200"]}], seed, [], point * 100n, 100n, 1)[0];
    assert.deepEqual(projection.target, {min: 10000n, max: 40000n});
    assert.equal(projection.reached, reached, `Range status at ${point} points`);
}
const fixed = projectFuturePoints(13001, [{level: 13001, target: "100"}], seed, [], 10000n, 100n, 1)[0];
assert.deepEqual(fixed.target, {min: 10000n, max: 10000n});
assert.equal(fixed.reached, true);
const unknown = projectFuturePoints(13001, [{level: 13001}], seed, [], 10000n, 100n, 1)[0];
assert.equal(unknown.target, null);
assert.equal(unknown.reached, null);
const hugeRange = projectFuturePoints(14199, [{level: 14199, target: ["9.5e400", "1.25e400"]}], seed, [], 5n * 10n ** 402n, 100n, 1)[0];
assert.deepEqual(hugeRange.target, {min: 125n * 10n ** 400n, max: 950n * 10n ** 400n});
assert.equal(hugeRange.reached, null, "Scientific-notation bounds beyond Number's limit must stay exact.");
console.log("Passed: exported stages, enemy target ranges and boundaries, noncombat skipping, exact large scores, QYZZ compatibility and battle-only growth.");
