import assert from "node:assert/strict";
import {collectFrontendDiagnostics} from "../src/lib/diagnostics";
import {useLogStore} from "../src/lib/logStore";

const preferences: Record<string, string> = {
    "sl-theme": "dark", "sl-lang": "ja-JP", "sl-custom-switch:wall-limit": "24",
    "sl-hud:show-current-level": "0",
    "account-token": "must-not-export", "cached-account": "must-not-export",
};
Object.defineProperty(globalThis, "localStorage", {configurable: true, value: {
    getItem: (key: string) => preferences[key] ?? null,
    key: () => {throw new Error("must not enumerate storage");},
}});
Object.defineProperty(globalThis, "navigator", {configurable: true, value: {userAgent: "test-webview", language: "zh-CN"}});
Object.defineProperty(globalThis, "window", {configurable: true, value: {innerWidth: 1000, innerHeight: 700, devicePixelRatio: 2}});
useLogStore.getState().addLog("ERROR", "frontend failure");
useLogStore.getState().addBackendLogs([{ts_ms: 1, level: "INFO", target: "backend", message: "backend entry"}]);
const result = collectFrontendDiagnostics();
assert.equal(result.settings["sl-theme"], "dark");
assert.equal(result.settings["sl-custom-switch:wall-limit"], "24");
assert.equal(result.settings["sl-lang"], "ja-JP");
assert.equal(result.settings["sl-hud:show-current-level"], "0");
assert.equal(result.display.scale, 2);
assert.equal(result.logs.length, 1);
assert.equal(result.logs[0].message, "frontend failure");
assert.ok(!JSON.stringify(result).includes("must-not-export"));
Object.defineProperty(globalThis, "localStorage", {configurable: true, get: () => {throw new Error("storage denied");}});
const restricted = collectFrontendDiagnostics();
assert.match(restricted.storage_error ?? "", /storage denied/);
assert.equal(restricted.user_agent, "test-webview");
console.log("diagnostics frontend assertions passed");
