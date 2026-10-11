import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

const plain = value => JSON.parse(JSON.stringify(value));
const defaults = {
    prefer_dora: false, prefer_soul: false, any_waits: false,
    preferred_suit: null, preferred_meld_type: null,
};

function evaluate(source, globals) {
    const exports = {};
    const code = ts.transpileModule(source, {
        compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
    }).outputText;
    runInNewContext(code, {exports, ...globals});
    return exports;
}

function settings(stored = new Map()) {
    return evaluate(readFileSync(new URL("../src/lib/wanxiangSettings.ts", import.meta.url), "utf8"), {
        localStorage: {getItem: key => stored.get(key) ?? null},
    });
}

test("Wanxiang preferences preserve saved choices and reject invalid stored values", () => {
    const stored = new Map();
    const api = settings(stored);
    assert.deepEqual(plain(api.readWanxiangPreferences()), defaults);
    stored.set("sl-wanxiang-switch:prefer-bonus", "1");
    for (const suit of ["m", "p", "s", "z"]) {
        for (const type of ["triplet", "sequence"]) {
            stored.set("sl-wanxiang-switch:preferred-suit", suit);
            stored.set("sl-wanxiang-switch:preferred-meld-type", type);
            assert.deepEqual(plain(api.readWanxiangPreferences()), {
                ...defaults, prefer_dora: true, prefer_soul: true, preferred_suit: suit, preferred_meld_type: type,
            });
        }
    }
    stored.set("sl-wanxiang-switch:prefer-bonus", "invalid");
    stored.set("sl-wanxiang-switch:preferred-suit", "invalid");
    stored.set("sl-wanxiang-switch:preferred-meld-type", "invalid");
    assert.deepEqual(plain(api.readWanxiangPreferences()), defaults);
    assert.deepEqual(plain(settings({get() {throw new Error("storage unavailable");}}).readWanxiangPreferences()), defaults);
});

test("HUD starts read the latest main-window preferences without changing execute or stop requests", () => {
    const stored = new Map();
    const api = settings(stored);
    const source = ts.createSourceFile("overlay.tsx",
        readFileSync(new URL("../src/overlay.tsx", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    const declarations = source.statements.filter(statement =>
        ts.isFunctionDeclaration(statement) && statement.name?.text === "sendWanxiangAction"
        || ts.isVariableStatement(statement) && statement.declarationList.declarations.some(d => d.name.getText(source) === "WANXIANG_SEARCH_ALGORITHM"));
    const requests = [];
    const {sendWanxiangAction} = evaluate(declarations.map(d => d.getText(source)).join("\n") + "\nexport {sendWanxiangAction};", {
        readWanxiangPreferences: api.readWanxiangPreferences,
        backendIpc: {runSwitch: request => requests.push(plain(request))},
    });
    sendWanxiangAction("start");
    assert.deepEqual(requests.pop().options.preferences, defaults);
    stored.set("sl-wanxiang-switch:prefer-bonus", "1");
    stored.set("sl-wanxiang-switch:preferred-suit", "z");
    stored.set("sl-wanxiang-switch:preferred-meld-type", "triplet");
    sendWanxiangAction("start");
    assert.deepEqual(requests.pop(), {action: "start", options: {
        wall_limit: 36, search_algorithm: "wanxiang_four_meld_switch",
        preferences: {...defaults, prefer_dora: true, prefer_soul: true, preferred_suit: "z", preferred_meld_type: "triplet"},
    }});
    sendWanxiangAction("execute_plan", "plan-123");
    assert.deepEqual(requests.pop(), {action: "execute_plan", options: {plan_id: "plan-123"}});
    sendWanxiangAction("stop");
    assert.deepEqual(requests.pop(), {action: "stop", options: {plan_id: ""}});
});
