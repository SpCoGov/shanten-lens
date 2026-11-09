import assert from "node:assert/strict";
import {HUD_SHOW_CURRENT_LEVEL_KEY, readHudShowCurrentLevel, writeHudShowCurrentLevel} from "./hudSettings";

const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
const saved = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", {configurable: true, value: {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
}});
try {
    assert.equal(readHudShowCurrentLevel(), true);
    for (const enabled of [false, true, false]) {
        writeHudShowCurrentLevel(enabled);
        assert.equal(saved.get(HUD_SHOW_CURRENT_LEVEL_KEY), enabled ? "1" : "0");
        assert.equal(readHudShowCurrentLevel(), enabled);
    }
    Object.defineProperty(globalThis, "localStorage", {configurable: true, get: () => {throw new Error("storage denied");}});
    assert.equal(readHudShowCurrentLevel(), true);
    assert.doesNotThrow(() => writeHudShowCurrentLevel(false));
} finally {
    if (original) Object.defineProperty(globalThis, "localStorage", original);
    else Reflect.deleteProperty(globalThis, "localStorage");
}
console.log("HUD current-level settings checks passed.");
