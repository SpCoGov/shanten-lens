import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

const exports = {};
runInNewContext(ts.transpileModule(readFileSync(new URL("../src/lib/customTarget.ts", import.meta.url), "utf8"), {
    compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
}).outputText, {exports});

test("target editing counts quads and rejects damaged persisted filters", () => {
    const groups = Array.from({length: 14}, exports.newTargetGroup);
    assert.equal(exports.targetSize(groups), 14);
    assert.equal(exports.validTargetGroups(groups), true);
    groups[0].rule.suits.push("m");
    assert.equal(groups[1].rule.suits.length, 0, "Rows must not share editable arrays");
    groups.splice(0, 3, {...exports.newTargetGroup(), quad: true});
    assert.equal(groups.length, 12);
    assert.equal(exports.targetSize(groups), 14);
    groups[1].rule.faces = ["0m", "7z"];
    groups[1].rule.ranks = [5, 7];
    groups[1].rule.red = true;
    assert.equal(exports.validTargetGroups(JSON.parse(JSON.stringify(groups))), true);
    for (const invalid of [null, {}, [{rule: null}], [{quad: true, rule: {...groups[1].rule, ranks: [0]}}],
        [{quad: false, rule: {...groups[1].rule, faces: ["8z"]}}], [{quad: false, rule: {...groups[1].rule, joker: "unknown"}}]]) {
        assert.equal(exports.validTargetGroups(invalid), false);
    }
});

test("saved per-slot joker preferences migrate without losing tile conditions", () => {
    const groups = Array.from({length: 14}, exports.newTargetGroup);
    groups[0].rule.joker = "only";
    groups[0].rule.faces = ["1m", "2m"];
    groups[1].rule.joker = "exclude";
    groups[1].rule.soul = true;
    groups[2].quad = true;
    const restored = exports.restoreTargetGroups(groups);
    assert.equal(restored[0].rule.joker, "allow");
    assert.equal(restored[1].rule.joker, "allow");
    assert.equal(restored[2].rule.joker, "exclude");
    assert.equal(JSON.stringify(restored[0].rule.faces), '["1m","2m"]');
    assert.equal(restored[1].rule.soul, true);
    assert.equal(groups[0].rule.joker, "only", "migration does not mutate its input");
    assert.equal(exports.targetSize(exports.restoreTargetGroups(null)), 14);
});

test("batch conditions replace selected rules while preserving group types and unrelated targets", () => {
    const groups = Array.from({length: 4}, exports.newTargetGroup);
    groups[0].rule.faces = ["0m", "5m"];
    groups[0].rule.suits = ["m"];
    groups[0].rule.ranks = [5];
    groups[0].rule.red = true;
    groups[1].quad = true;
    groups[1].rule.suits = ["z"];
    groups[1].rule.dora = true;
    const next = exports.applyTargetRule(groups, [0, 1, 3], groups[0].rule);
    assert.equal(next[1].quad, true);
    assert.equal(next[1].rule.joker, "exclude");
    assert.equal(next[3].quad, false);
    assert.equal(next[3].rule.joker, "allow");
    assert.equal(JSON.stringify(next[1].rule.faces), '["0m","5m"]');
    assert.equal(next[1].rule.red, true);
    assert.equal(next[1].rule.dora, false, "replace old conditions rather than merging them");
    assert.equal(next[2], groups[2], "unselected targets are untouched");
    assert.equal(groups[1].rule.dora, true, "source targets are not mutated");
    for (const key of ["faces", "suits", "ranks"]) {
        assert.notEqual(next[0].rule[key], next[1].rule[key]);
        assert.notEqual(next[0].rule[key], groups[0].rule[key]);
    }
    assert.equal(exports.targetSize(next), exports.targetSize(groups));
    assert.equal(exports.applyTargetRule(groups, [], groups[0].rule)[0], groups[0]);
});

test("the custom editor's static labels and result messages exist in both languages", () => {
    const page = readFileSync(new URL("../src/pages/CustomSwitchPage.tsx", import.meta.url), "utf8");
    for (const language of ["zh-CN", "ja-JP"]) {
        const locale = JSON.parse(readFileSync(new URL(`../src/locales/${language}.json`, import.meta.url), "utf8"));
        for (const [, key] of page.matchAll(/t\("([a-z_]+\.[a-z_]+)"/g)) {
            const value = key.split(".").reduce((current, part) => current?.[part], locale);
            assert.ok(typeof value === "string", `${language}: ${key}`);
        }
        for (const kind of ["exchange", "draw", "discard", "kan"]) assert.ok(locale.custom_target[`step_${kind}`]);
        for (const key of ["joker_present", "joker_absent"]) assert.ok(locale.custom_target[key]);
        for (const key of ["execute_reason_state_mismatch", "execute_reason_game_ended"]) assert.ok(locale.blackhole[key]);
        for (const reason of ["invalid-target", "invalid-options", "hand-size", "joker-state", "wall-limit", "not-reachable", "search-limit"]) {
            assert.ok(locale.custom_target[`reason_custom-${reason}`]);
        }
    }
});

test("custom controls execute live plans and persist, transmit and invalidate the legal-shape filter", async () => {
    const states = [], requests = [], effects = [], stored = new Map();
    let cursor = 0, complete, cleared = 0;
    const react = {
        createElement: (type, props, ...children) => ({type, props: props ?? {}, children}),
        useEffect(effect) {effects.push(effect);},
        useState(initial) {
            const index = cursor++;
            if (!(index in states)) states[index] = typeof initial === "function" ? initial() : initial;
            return [states[index], value => {states[index] = typeof value === "function" ? value(states[index]) : value;}];
        },
    };
    const pageExports = {};
    const mocks = {
        react: {default: react}, "react-i18next": {useTranslation: () => ({t: key => key})},
        "../components/Tile": {default: "Tile"}, "../lib/customTarget": exports,
        "./CustomSwitchPage.module.css": {default: {}},
        "../lib/ipc": {runSwitch: request => {
            requests.push(JSON.parse(JSON.stringify(request)));
            return new Promise(resolve => {complete = resolve;});
        }},
    };
    runInNewContext(ts.transpileModule(readFileSync(new URL("../src/pages/CustomSwitchPage.tsx", import.meta.url), "utf8"), {
        compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.React},
    }).outputText, {exports: pageExports, localStorage: {
        getItem: key => stored.get(key) ?? null, setItem: (key, value) => stored.set(key, value),
    }, require: name => mocks[name]});
    const state = {stage: 4, revision: 12, hand_tiles: [], deck_map: {}};
    const plan = {status: "plan", state_revision: 12, plan_id: "live-custom-1", request_source: "live", search_algorithm: "custom_target"};
    function render(currentState = state, data = plan) {
        cursor = 0;
        const found = new Map();
        function visit(node) {
            if (Array.isArray(node)) return node.forEach(visit);
            if (!node || typeof node !== "object") return;
            if (node.type === "button") {
                for (const label of ["blackhole.execute_plan", "blackhole.execute_full_plan", "custom_target.start"]) {
                    if (node.children.includes(label)) found.set(label, node.props);
                }
            }
            if (node.type === "label" && node.props.title === "custom_target.exclude_invalid_shapes_hint") {
                found.set("custom_target.exclude_invalid_shapes", node.children.find(child => child.type === "input").props);
            }
            node.children?.forEach(visit);
        }
        visit(pageExports.default({currentState, data, onClear() {cleared++;}}));
        effects.splice(0).forEach(effect => effect());
        return found;
    }
    for (const [current, data] of [[null, plan], [{...state, stage: 0}, plan], [{...state, revision: 13}, plan],
        [state, null], [state, {...plan, plan_id: ""}], [state, {...plan, request_source: "debug"}],
        [state, {...plan, search_algorithm: "target_enumeration_search"}], [state, {...plan, status: "searching"}]]) {
        for (const key of ["blackhole.execute_plan", "blackhole.execute_full_plan"]) {
            const button = render(current, data).get(key);
            assert.equal(button.disabled, true);
            await button.onClick();
            assert.equal(requests.length, 0);
        }
    }
    for (const action of ["execute_plan", "execute_full_plan"]) {
        const button = render().get(`blackhole.${action}`);
        assert.equal(button.disabled, false);
        button.onClick();
        assert.deepEqual(requests.at(-1), {action, options: {plan_id: plan.plan_id}});
        for (const key of ["blackhole.execute_plan", "blackhole.execute_full_plan", "custom_target.start", "custom_target.exclude_invalid_shapes"]) {
            assert.equal(render().get(key).disabled, true);
        }
        complete();
        await new Promise(resolve => setImmediate(resolve));
        assert.equal(render().get(`blackhole.${action}`).disabled, false);
    }
    const option = "custom_target.exclude_invalid_shapes";
    const storageKey = "sl-custom-switch:exclude-invalid-shapes";
    assert.equal(render().get(option).checked, false);
    render().get("custom_target.start").onClick();
    assert.equal(requests.at(-1).options.exclude_invalid_shapes, false);
    complete();
    const beforeToggle = cleared;
    render().get(option).onChange({target: {checked: true}});
    assert.equal(cleared, beforeToggle + 1, "Changing the filter clears the old plan");
    assert.equal(render().get(option).checked, true);
    assert.equal(stored.get(storageKey), "1");
    render().get("custom_target.start").onClick();
    assert.equal(requests.at(-1).options.exclude_invalid_shapes, true);
    complete();
    states.length = 0;
    assert.equal(render().get(option).checked, true, "A remount restores the filter");
    render(state, {...plan, status: "searching"}).get(option).onChange({target: {checked: false}});
    assert.deepEqual(requests.at(-1), {action: "stop", notify: false});
    complete();
    assert.equal(render().get(option).checked, false);
    assert.equal(stored.get(storageKey), "0");
});
