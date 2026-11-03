import assert from "node:assert/strict";
import {newTargetGroup, setTargetGroupRequirement, setTargetGroupKind, targetKind, validTargetGroups,
    compactTargetPreset, parseTargetPreset, restoreTargetGroups, type TargetPreset} from "../src/lib/customTarget";

const original = Array.from({length: 14}, newTargetGroup);
let groups = setTargetGroupRequirement(original, 0, "require_draw", true);
assert.equal(groups[0].require_draw, true);
assert.equal(original[0].require_draw, undefined);
groups = setTargetGroupRequirement(groups, 1, "require_draw", true);
assert.equal(groups[0].require_draw, false);
assert.equal(groups[1].require_draw, true);
groups = setTargetGroupRequirement(groups, 1, "require_joker", true);
assert.equal(groups[1].require_draw, false);
assert.equal(groups[1].require_joker, true);
groups = setTargetGroupRequirement(groups, 1, "require_draw", true);
assert.equal(groups[1].require_joker, false);
groups = setTargetGroupKind(groups, [1], "pair");
groups = setTargetGroupRequirement(groups, 1, "require_joker", true);
assert.equal(groups[1].require_draw, true);
assert.equal(groups[1].require_joker, true);
assert.equal(validTargetGroups(groups), true);
groups = setTargetGroupRequirement(groups, 2, "require_joker", true);
assert.equal(groups[1].require_joker, false);
assert.equal(groups[2].require_joker, true);
groups = setTargetGroupRequirement(groups, 1, "require_joker", true);
const compact = compactTargetPreset({format:"shanten-lens.custom-target",version:1,name:"Roles",target_groups:groups,wall_limit:36,exclude_invalid_shapes:false});
const loaded = parseTargetPreset(JSON.parse(JSON.stringify(compact)))!;
assert.equal(loaded.target_groups[1].require_draw, true);
assert.equal(loaded.target_groups[1].require_joker, true);
assert.deepEqual(restoreTargetGroups(loaded.target_groups), loaded.target_groups);
assert.equal(compact.target_groups[0].require_draw, undefined);
const single = setTargetGroupKind(groups, [1], "single");
assert.equal(targetKind(single[1]), "single");
assert.equal(single[1].require_draw, true);
assert.equal(single[1].require_joker, false);
assert.equal(validTargetGroups(single), true);
for (const key of ["require_draw", "require_joker"] as const) {
    const duplicate = structuredClone(groups); duplicate[3][key] = true;
    assert.equal(validTargetGroups(duplicate), false);
    const invalid = structuredClone(groups); Object.assign(invalid[1], {[key]: "yes"});
    assert.equal(validTargetGroups(invalid), false);
}
const invalidSingle = structuredClone(original);
invalidSingle[0].require_draw = true; invalidSingle[0].require_joker = true;
assert.equal(validTargetGroups(invalidSingle), false);
assert.equal(parseTargetPreset({...compact,target_groups:invalidSingle}), null);
assert.equal(validTargetGroups(original), true);
console.log("Custom target group requirement assertions passed");
