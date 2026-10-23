import assert from "node:assert/strict";
import {newTargetGroup} from "./customTarget";
import {readCustomSwitchOptions, CUSTOM_SWITCH_TARGET_KEY, CUSTOM_SWITCH_WALL_LIMIT_KEY, CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY} from "./customSwitchSettings";
import {readHudShowCustomSwitch, writeHudShowCustomSwitch} from "./hudSettings";

const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
const saved = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {configurable: true, value: {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
}});
try {
    const defaults = readCustomSwitchOptions();
    assert.equal(defaults.search_algorithm, "custom_target");
    assert.equal(defaults.target_groups.length, 14);
    assert.equal(defaults.wall_limit, 36);
    assert.equal(defaults.exclude_invalid_shapes, false);
    const groups = Array.from({length: 14}, newTargetGroup);
    groups[0].rule.faces = ["3m"];
    saved.set(CUSTOM_SWITCH_TARGET_KEY, JSON.stringify(groups));
    saved.set(CUSTOM_SWITCH_WALL_LIMIT_KEY, "0");
    saved.set(CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY, "1");
    assert.deepEqual(readCustomSwitchOptions(), {search_algorithm: "custom_target", target_groups: groups,
        wall_limit: 0, exclude_invalid_shapes: true});
    groups[0].rule.faces = ["5p"];
    saved.set(CUSTOM_SWITCH_TARGET_KEY, JSON.stringify(groups));
    saved.set(CUSTOM_SWITCH_WALL_LIMIT_KEY, "24");
    assert.deepEqual(readCustomSwitchOptions().target_groups, groups);
    assert.equal(readCustomSwitchOptions().wall_limit, 24);
    for (const bad of ["NaN", "-1", "37", "1.5"]) {
        saved.set(CUSTOM_SWITCH_WALL_LIMIT_KEY, bad);
        assert.equal(readCustomSwitchOptions().wall_limit, 36);
    }
    saved.set(CUSTOM_SWITCH_TARGET_KEY, "malformed");
    assert.equal(readCustomSwitchOptions().target_groups.length, 14);
    assert.equal(readHudShowCustomSwitch(), false);
    writeHudShowCustomSwitch(true);
    assert.equal(readHudShowCustomSwitch(), true);
    writeHudShowCustomSwitch(false);
    assert.equal(readHudShowCustomSwitch(), false);
} finally {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else Reflect.deleteProperty(globalThis, "localStorage");
}
