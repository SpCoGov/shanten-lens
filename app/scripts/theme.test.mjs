import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/lib/theme.ts", import.meta.url), "utf8"), {
    compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
}).outputText;
const plain = (value) => JSON.parse(JSON.stringify(value));

function setup({theme, custom, dark = false, random = Math.random} = {}) {
    const stored = new Map();
    if (theme !== undefined) stored.set("sl-theme", theme);
    if (custom !== undefined) stored.set("sl-custom-theme", JSON.stringify(custom));
    const attributes = new Map(), properties = new Map();
    const root = {
        setAttribute: (key, value) => attributes.set(key, value),
        removeAttribute: (key) => attributes.delete(key),
        getAttribute: (key) => attributes.get(key) ?? null,
        style: {
            setProperty: (key, value) => properties.set(key, value),
            removeProperty: (key) => properties.delete(key),
            getPropertyValue: (key) => properties.get(key) ?? "",
        },
    };
    const window = new EventTarget(), media = new EventTarget();
    media.matches = dark;
    window.matchMedia = () => media;
    const localStorage = {
        getItem: (key) => stored.get(key) ?? null,
        setItem: (key, value) => stored.set(key, String(value)),
    };
    const api = {};
    runInNewContext(code, {exports: api, localStorage, document: {documentElement: root}, window, Event,
        Math: Object.assign(Object.create(Math), {random})});
    return {
        api, stored, root, properties, localStorage, window,
        storage: (key) => window.dispatchEvent(Object.assign(new Event("storage"), {key})),
        system: (matches) => { media.matches = matches; media.dispatchEvent(new Event("change")); },
    };
}

test("random palettes preserve the base, cover all colors and keep text readable across hues", () => {
    const luminance = (hex) => hex.slice(1).match(/../g).map((part) => {
        const value = parseInt(part, 16) / 255;
        return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    }).reduce((sum, channel, index) => sum + channel * [0.2126, 0.7152, 0.0722][index], 0);
    let sample = 0;
    const {api, stored, root} = setup({theme: "custom", random: () => sample});
    for (const base of ["light", "dark"]) {
        const palettes = new Set();
        for (let hue = 0; hue < 360; hue++) {
            sample = hue / 360;
            const theme = api.createRandomTheme(base);
            assert.equal(theme.base, base);
            assert.deepEqual(Object.keys(theme.colors).sort(), Array.from(api.CUSTOM_THEME_COLORS, ({key}) => key).sort());
            for (const color of Object.values(theme.colors)) assert.match(color, /^#[0-9a-f]{6}$/);
            for (const foreground of ["--color-text", "--color-muted", "--color-ring"]) {
                for (const background of ["--color-bg", "--panel-bg", "--input-bg"]) {
                    const values = [luminance(theme.colors[foreground]), luminance(theme.colors[background])].sort((a, b) => a - b);
                    assert.ok((values[1] + 0.05) / (values[0] + 0.05) >= 4.5, `${base}: ${hue}, ${foreground} on ${background}`);
                }
            }
            palettes.add(JSON.stringify(theme.colors));
        }
        assert.ok(palettes.size > 300);
        const theme = api.createRandomTheme(base);
        api.setCustomTheme(theme);
        assert.deepEqual(JSON.parse(stored.get("sl-custom-theme")), plain(theme));
        assert.equal(root.style.getPropertyValue("--color-ring"), theme.colors["--color-ring"]);
    }
});

test("saved modes remain compatible and unknown or inaccessible settings fall back to auto", () => {
    const {api, stored, localStorage} = setup();
    for (const mode of ["auto", "dark", "dark-green", "dark-purple", "custom"]) {
        stored.set("sl-theme", mode);
        assert.equal(api.readTheme(), mode);
    }
    for (const mode of ["light", "", "broken"]) {
        stored.set("sl-theme", mode);
        assert.equal(api.readTheme(), "auto");
    }
    stored.clear();
    assert.equal(api.readTheme(), "auto");
    localStorage.getItem = () => { throw new Error("storage unavailable"); };
    assert.equal(api.readTheme(), "auto");
    assert.deepEqual(plain(api.readCustomTheme()), {base: "dark", colors: {}});
});

test("custom settings reject malformed data, unknown variables and non-hex CSS", () => {
    const {api, stored} = setup();
    for (const raw of ["{", "null", "[]", "12", '"dark"', '{"base":"unknown","colors":null}']) {
        stored.set("sl-custom-theme", raw);
        assert.deepEqual(plain(api.readCustomTheme()), {base: "dark", colors: {}});
    }
    stored.set("sl-custom-theme", JSON.stringify({
        base: "light",
        colors: {
            "--color-bg": "#AbCdEf",
            "--color-text": "#012345",
            "--panel-bg": "url(https://example.com/image)",
            "--color-muted": "#123",
            "--color-border": "#12345678",
            "--color-ring": "red",
            "--input-bg": 123456,
            "--state-ok": " #123456",
            "--state-warn": "#123456; color:red",
            "--state-down": "var(--color-text)",
            "--untrusted-property": "#123456",
        },
    }));
    assert.deepEqual(plain(api.readCustomTheme()), {
        base: "light", colors: {"--color-bg": "#AbCdEf", "--color-text": "#012345"},
    });
});

test("saving custom settings persists only the supported color fields", () => {
    const {api, stored} = setup();
    const colors = Object.fromEntries(api.CUSTOM_THEME_COLORS.map(({key}) => [key, "#102030"]));
    api.setCustomTheme({base: "light", colors: {...colors, "--unknown": "#ffffff"}, extra: true});
    assert.deepEqual(JSON.parse(stored.get("sl-custom-theme")), {base: "light", colors});
});

test("custom light and dark bases override the system preference and preset themes clear colors", () => {
    for (const base of ["light", "dark"]) {
        const {api, root, properties} = setup({
            dark: base === "light",
            theme: "custom",
            custom: {base, colors: {"--color-bg": "#123456", "--color-ring": "#abcdef"}},
        });
        root.style.setProperty("--theme-wave-x", "20px");
        api.applyTheme("custom");
        assert.equal(root.getAttribute("data-theme"), base === "dark" ? "dark" : null);
        assert.equal(root.getAttribute("data-custom-theme"), base);
        assert.equal(root.style.getPropertyValue("--color-bg"), "#123456");
        assert.equal(root.style.getPropertyValue("--color-ring"), "#abcdef");
        api.applyTheme("dark-green");
        assert.equal(root.getAttribute("data-theme"), "dark-green");
        assert.equal(root.getAttribute("data-custom-theme"), null);
        assert.deepEqual([...properties], [["--theme-wave-x", "20px"]]);
    }
});

test("replacing a custom palette removes colors omitted from the new palette", () => {
    const {api, root} = setup({theme: "custom"});
    api.setCustomTheme({base: "dark", colors: {"--color-bg": "#123456", "--color-ring": "#abcdef"}});
    api.setCustomTheme({base: "light", colors: {"--color-bg": "#ffffff"}});
    assert.equal(root.getAttribute("data-theme"), null);
    assert.equal(root.getAttribute("data-custom-theme"), "light");
    assert.equal(root.style.getPropertyValue("--color-bg"), "#ffffff");
    assert.equal(root.style.getPropertyValue("--color-ring"), "");
});

test("theme subscriptions filter storage keys and unsubscribe both event sources", () => {
    const {api, storage, window} = setup();
    let calls = 0;
    const unsubscribe = api.subscribeThemeChanges(() => calls++);
    storage("unrelated");
    assert.equal(calls, 0);
    storage("sl-theme");
    storage("sl-custom-theme");
    storage(null);
    window.dispatchEvent(new Event("sl:theme-change"));
    assert.equal(calls, 4);
    unsubscribe();
    storage("sl-theme");
    window.dispatchEvent(new Event("sl:theme-change"));
    assert.equal(calls, 4);
});

test("initialization follows local, cross-window and system changes, then removes its listeners", () => {
    const {api, stored, root, storage, system, window} = setup({theme: "auto"});
    const cleanup = api.initializeTheme();
    assert.equal(root.getAttribute("data-theme"), null);
    system(true);
    assert.equal(root.getAttribute("data-theme"), "dark");
    api.setTheme("custom");
    api.setCustomTheme({base: "light", colors: {"--color-bg": "#abcdef"}});
    assert.equal(root.getAttribute("data-theme"), null);
    system(false);
    system(true);
    assert.equal(root.getAttribute("data-theme"), null);
    stored.set("sl-custom-theme", JSON.stringify({base: "dark", colors: {"--color-bg": "#123456"}}));
    storage("sl-custom-theme");
    assert.equal(root.getAttribute("data-theme"), "dark");
    assert.equal(root.style.getPropertyValue("--color-bg"), "#123456");
    stored.set("sl-theme", "dark-purple");
    storage("sl-theme");
    assert.equal(root.getAttribute("data-theme"), "dark-purple");
    assert.equal(root.style.getPropertyValue("--color-bg"), "");
    stored.set("sl-theme", "dark-green");
    window.dispatchEvent(new Event("sl:theme-change"));
    assert.equal(root.getAttribute("data-theme"), "dark-green");
    stored.clear();
    storage(null);
    assert.equal(root.getAttribute("data-theme"), "dark");
    system(false);
    assert.equal(root.getAttribute("data-theme"), null);
    cleanup();
    system(true);
    stored.set("sl-theme", "dark-purple");
    storage("sl-theme");
    window.dispatchEvent(new Event("sl:theme-change"));
    assert.equal(root.getAttribute("data-theme"), null);
});

test("failed storage writes retain the saved and applied theme without announcing a change", () => {
    const {api, root, stored, localStorage} = setup({
        theme: "custom", custom: {base: "light", colors: {"--color-bg": "#abcdef"}},
    });
    api.initializeTheme();
    let changes = 0;
    api.subscribeThemeChanges(() => changes++);
    localStorage.setItem = () => { throw new Error("quota exceeded"); };
    assert.throws(() => api.setTheme("dark-green"), /quota exceeded/);
    assert.throws(() => api.setCustomTheme({base: "dark", colors: {"--color-bg": "#123456"}}), /quota exceeded/);
    assert.equal(stored.get("sl-theme"), "custom");
    assert.equal(JSON.parse(stored.get("sl-custom-theme")).colors["--color-bg"], "#abcdef");
    assert.equal(root.getAttribute("data-theme"), null);
    assert.equal(root.getAttribute("data-custom-theme"), "light");
    assert.equal(root.style.getPropertyValue("--color-bg"), "#abcdef");
    assert.equal(changes, 0);
});

test("message-box preload matches runtime themes, custom palettes and malformed settings", () => {
    const preload = readFileSync(new URL("../public/theme-preload.js", import.meta.url), "utf8");
    const cases = ["auto", "dark", "dark-green", "dark-purple", "invalid", undefined].map((theme) => ({theme}));
    for (const base of ["light", "dark"]) {
        const colors = Object.fromEntries(setup().api.CUSTOM_THEME_COLORS.map(({key}) => [key, "#AbCdEf"]));
        cases.push({theme: "custom", custom: {base, colors: {...colors, "--unknown": "#123456"}}});
    }
    cases.push({theme: "custom", custom: {base: "invalid", colors: {"--color-bg": "url(example)"}}});
    cases.push({theme: "custom", raw: "{"}, {theme: "custom", raw: "null"});
    for (const dark of [false, true]) {
        for (const scenario of cases) {
            const runtime = setup({...scenario, dark}), early = setup({...scenario, dark});
            if (scenario.raw !== undefined) {
                runtime.stored.set("sl-custom-theme", scenario.raw);
                early.stored.set("sl-custom-theme", scenario.raw);
            }
            runtime.api.applyTheme(runtime.api.readTheme());
            runInNewContext(preload, {document: {documentElement: early.root}, localStorage: early.localStorage, window: early.window});
            const context = JSON.stringify({...scenario, dark});
            for (const key of ["data-theme", "data-custom-theme"]) {
                assert.equal(early.root.getAttribute(key), runtime.root.getAttribute(key), context);
            }
            assert.deepEqual([...early.properties], [...runtime.properties], context);
        }
    }
});
