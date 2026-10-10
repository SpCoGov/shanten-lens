import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

const exports = {};
runInNewContext(ts.transpileModule(readFileSync(new URL("../src/lib/sidebarLayout.ts", import.meta.url), "utf8"), {
    compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
}).outputText, {exports});

test("saved navigation drops retired test pages while preserving order and unavailable plugins", () => {
    const defaults = {outside: ["home", "more", "spacer"], more: ["blackhole", "wanxiang", "custom-switch"]};
    const saved = {
        outside: ["souzu-debug", "wanxiang", "frontend-test", "home", "more", "spacer"],
        more: ["plugin:disabled", "separator:debug", "frontend-test", "souzu-debug", "blackhole"],
    };
    const expected = {
        outside: ["wanxiang", "home", "more", "spacer"],
        more: ["plugin:disabled", "blackhole", "custom-switch"],
    };
    const actual = exports.normalizeSidebarLayout(saved, defaults);
    assert.deepEqual(JSON.parse(JSON.stringify(actual)), expected);
    assert.deepEqual(JSON.parse(JSON.stringify(exports.normalizeSidebarLayout(actual, defaults))), expected);
    assert.deepEqual(JSON.parse(JSON.stringify(exports.normalizeSidebarLayout(null, defaults))), defaults);
    assert.ok(saved.outside.includes("frontend-test"), "normalization must not mutate the saved input");
});
