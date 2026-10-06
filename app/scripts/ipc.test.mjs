import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

test("switch commands report failures and prevent duplicate searches until completion", async () => {
    const calls = [], logs = [], toasts = [], results = [];
    let complete, fail;
    const commands = {backendSwitch: (request) => {
        calls.push(request);
        return new Promise((resolve, reject) => { complete = resolve; fail = reject; });
    }};
    const mocks = {
        "@tauri-apps/api/event": {listen: async () => () => {}},
        "../bindings": {commands},
        "./logStore": {useLogStore: {getState: () => ({addLog: (...args) => logs.push(args)})}},
        "./registryStore": {}, "./fuseStore": {}, "./autoRunnerStore": {},
        "./toast": {pushToast: (...args) => toasts.push(args)},
        i18next: {t: (key, values) => `${key}: ${values.reason}`},
    };
    const exports = {};
    const code = ts.transpileModule(readFileSync(new URL("../src/lib/ipc.ts", import.meta.url), "utf8"), {
        compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
    }).outputText;
    runInNewContext(code, {exports, require: (name) => {
        assert.ok(name in mocks, `Unexpected import: ${name}`);
        return mocks[name];
    }});
    await exports.onBackendEvent("discard_recommendation", (value) => results.push(value));

    const request = {action: "start", options: {search_algorithm: "target_enumeration_search"}};
    const first = exports.runSwitch(request);
    await exports.runSwitch(request);
    assert.equal(calls.length, 1);
    fail("IPC unavailable");
    await first;
    assert.equal(logs[0][1], "IPC unavailable");
    assert.equal(toasts[0][0], "blackhole.control_failed: IPC unavailable");
    assert.equal(results[0][0].data.status, "impossible");
    assert.equal(results[0][0].data.request_source, "live");
    assert.equal(results[0][0].data.search_algorithm, request.options.search_algorithm);

    const retry = exports.runSwitch(request);
    assert.equal(calls.length, 2);
    const finishSearch = complete;
    const stop = exports.runSwitch({action: "stop"});
    assert.equal(calls.length, 3, "Stop remains available during a search");
    complete(null);
    await stop;
    const restarted = exports.runSwitch(request);
    assert.equal(calls.length, 4, "Stopping allows a new search immediately");
    const finishRestarted = complete;
    finishSearch(null);
    await retry;
    await exports.runSwitch(request);
    assert.equal(calls.length, 4, "An old completion must not unlock the new search");
    finishRestarted(null);
    await restarted;
    assert.equal(results.length, 1, "Successful calls rely on backend result events");

    const debug = exports.runSwitch({action: "start_debug"});
    fail("debug failed");
    await debug;
    assert.equal(results[1][0].data.request_source, "debug");
    for (const language of ["zh-CN", "ja-JP"]) {
        const locale = JSON.parse(readFileSync(new URL(`../src/locales/${language}.json`, import.meta.url), "utf8"));
        assert.ok(locale.blackhole.control_failed.includes("{{reason}}"));
    }
});
