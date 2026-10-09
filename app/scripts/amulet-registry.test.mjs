import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";

test("IPC amulets refresh cards and prices without replacing the packet cache", () => {
    const cache = new Map();
    let updates = 0;
    const localStorage = {getItem: (key) => cache.get(key), setItem: (key, value) => cache.set(key, value)};
    function load(file, mocks) {
        const exports = {};
        const code = ts.transpileModule(readFileSync(new URL(file, import.meta.url), "utf8"), {
            compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX},
        }).outputText;
        runInNewContext(code, {exports, localStorage, require: (name) => {
            assert.ok(name in mocks, `Unexpected import: ${name}`);
            return mocks[name];
        }});
        return exports;
    }
    const react = {useSyncExternalStore: (subscribe, getSnapshot) => {
        subscribe(() => { updates++; });
        return getSnapshot();
    }};
    const registry = load("../src/lib/registryStore.ts", {react});
    const price = load("../src/lib/amuletPrice.ts", {"./registryStore": registry});
    const jsx = (type, props) => ({type, props});
    const card = load("../src/components/AmuletCard.tsx", {
        "react/jsx-runtime": {jsx, jsxs: jsx}, "../styles/theme.css": {},
        "../lib/registryStore": registry, "../lib/amuletPrice": price,
        i18next: {t: (key, values) => key === "amulet_card.title_known" ? values.name + values.suffix
            : key === "amulet_card.plus_suffix" ? "+" : key},
    }).default;
    const packet = {
        amulets: [{id: 167, icon_id: 155, name: "packet name", rarity: "ORANGE"}],
        badges: [{id: 600050, icon_id: 50, name: "packet badge", rarity: "RED"}],
    };
    registry.setRegistry(packet);
    const cachedPacket = cache.get("shanten:registry:v1");
    assert.equal(price.calcAmuletPrice({id: 1670}), 9);
    const item = {id: 1671, uid: 1, volume: 1, tags: [], store: []};
    assert.equal(card({item}).props.title, "packet name+");

    const catalog = JSON.parse(readFileSync(new URL("../../backend/assets/qyzz_amulets.json", import.meta.url), "utf8"));
    const localAmulet = catalog.amulets.find((amulet) => amulet.data_id === 1670);
    const local = {
        source: "qyzz", badges: packet.badges,
        amulets: [{id: 167, icon_id: 155, rarity: "ORANGE", name: localAmulet.name,
            plus_name: localAmulet.plus_name, sell_price: localAmulet.sell_price, plus_sell_price: localAmulet.plus_sell_price}],
    };
    registry.setRegistry(local);
    assert.ok(updates > 0, "Existing cards subscribe to registry changes");
    assert.equal(registry.getAmulet(167).name, "假面酒侍白夜");
    assert.equal(registry.getBadge(600050).name, "packet badge");
    assert.equal(card({item}).props.title, localAmulet.plus_name);
    assert.equal(price.calcAmuletPrice({id: 1670}), 50);
    assert.equal(price.calcAmuletPrice({id: 1671}), 50);
    assert.equal(price.calcAmuletPrice({id: 1671, badge: {id: 600050}}), 150);
    assert.equal(price.calcAmuletPrice({id: 2280}), 0);
    assert.equal(cache.get("shanten:registry:v1"), cachedPacket);
    const restarted = load("../src/lib/registryStore.ts", {react});
    assert.equal(restarted.getAmulet(167).name, "packet name");

    registry.setRegistry(packet);
    assert.equal(card({item}).props.title, "packet name+");
    assert.equal(price.calcAmuletPrice({id: 1671}), 9);
    assert.equal(cache.get("shanten:registry:v1"), cachedPacket);
});
