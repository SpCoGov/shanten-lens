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
    const languageListeners = new Set();
    const i18n = {
        language: "zh-CN", resolvedLanguage: "zh-CN",
        on: (event, listener) => { assert.equal(event, "languageChanged"); languageListeners.add(listener); },
        changeLanguage: (language) => {
            i18n.language = i18n.resolvedLanguage = language;
            languageListeners.forEach((listener) => listener(language));
        },
    };
    const registryMocks = {react, i18next: {default: i18n}};
    const registry = load("../src/lib/registryStore.ts", registryMocks);
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
    const restarted = load("../src/lib/registryStore.ts", registryMocks);
    assert.equal(restarted.getAmulet(167).name, "packet name");

    registry.setRegistry(packet);
    assert.equal(card({item}).props.title, "packet name+");
    assert.equal(price.calcAmuletPrice({id: 1671}), 9);
    assert.equal(cache.get("shanten:registry:v1"), cachedPacket);

    const metadata = {
        data_id: 1670, icon_id: 155, icon_name: "fu_0155.png", deprecated: false, rarity: "ORANGE",
        names: {chs: "护符", chs_t: "護符", jp: "お守り", en: "Amulet", kr: "부적"},
        descriptions: {chs: "描述", chs_t: "描述", jp: "説明", en: "<b>{0}</b>", kr: ""},
        price: 18, sell_price: 9,
    };
    const rich = {
        amulets: [{...packet.amulets[0], metadata, plus_name: "legacy plus", plus_metadata: {...metadata,
            names: {chs: "护符+", chs_t: "護符+", jp: "お守り+", en: "Amulet+", kr: ""}}},
            {...packet.amulets[0], id: 168, metadata: {...metadata, deprecated: true}}],
        badges: [{...packet.badges[0], metadata: {...metadata, price: null, sell_price: null,
            names: {chs: "印章", chs_t: "印章", jp: "印", en: "Seal", kr: ""}}},
            {...packet.badges[0], id: 600060, metadata: {...metadata, deprecated: true}}],
    };
    registry.setRegistry(rich);
    const restored = load("../src/lib/registryStore.ts", registryMocks);
    assert.equal(JSON.stringify(restored.getAmulet(167).metadata), JSON.stringify(metadata));
    assert.equal(restored.getBadge(600050).metadata.sell_price, null);
    assert.equal(restored.getAmulet(168).metadata.deprecated, true);
    const rawCache = cache.get("shanten:registry:v1");
    for (const [language, baseName, plusName, badgeName] of [
        ["zh-CN", "护符", "护符+", "印章"],
        ["ja-JP", "お守り", "お守り+", "印"],
        ["zh-Hant-TW", "護符", "護符+", "印章"],
        ["zh_HK", "護符", "護符+", "印章"],
        ["en-US", "Amulet", "Amulet+", "Seal"],
        ["ko-KR", "부적", "legacy plus", "packet badge"],
    ]) {
        const before = registry.getRegistry();
        i18n.changeLanguage(language);
        assert.notEqual(registry.getRegistry(), before, "Language changes publish a new snapshot");
        assert.equal(registry.getAmulet(167).name, baseName);
        assert.equal(registry.getAmulet(167).plus_name, plusName);
        assert.equal(registry.getBadge(600050).name, badgeName);
        assert.equal(card({item}).props.title, plusName);
        assert.equal(price.calcAmuletPrice({id: 1670}), 9);
        assert.equal(cache.get("shanten:registry:v1"), rawCache, "Localized names never replace canonical cached data");
        assert.equal(rich.amulets[0].name, "packet name");
    }
    registry.setRegistry(local);
    i18n.changeLanguage("ja-JP");
    assert.equal(registry.getAmulet(167).name, localAmulet.name, "Custom entries without translations retain their source name");
    assert.equal(cache.get("shanten:registry:v1"), rawCache);
    const translatedLocal = {...local, amulets: [{...local.amulets[0], names: {...metadata.names, chs: localAmulet.name},
        plus_names: {...metadata.names, chs: localAmulet.plus_name, jp: "local-ja-plus"}}]};
    registry.setRegistry(translatedLocal);
    assert.equal(registry.getAmulet(167).name, "お守り");
    assert.equal(card({item}).props.title, "local-ja-plus");
    i18n.changeLanguage("zh-CN");
    assert.equal(registry.getAmulet(167).name, localAmulet.name);
    assert.equal(card({item}).props.title, localAmulet.plus_name);
    assert.equal(price.calcAmuletPrice({id: 1671}), 50);
    assert.equal(cache.get("shanten:registry:v1"), rawCache);
    registry.setRegistry(rich);
    i18n.changeLanguage("zh-CN");
    const uiReact = {useState: (value) => [value, () => {}], useEffect: () => {}};
    for (const kind of ["Amulet", "Badge"]) {
        const picker = load(`../src/components/${kind}PickerModal.tsx`, {
            react: {default: uiReact}, "react/jsx-runtime": {jsx, jsxs: jsx}, "../styles/theme.css": {},
            "./Modal": {default: "modal"}, "../lib/registryStore": {useRegistry: registry.getRegistry},
            "react-i18next": {useTranslation: () => ({t: (key) => key})},
        }).default;
        const images = [];
        const visit = (node) => {
            if (Array.isArray(node)) return node.forEach(visit);
            if (node?.type === "img") images.push(node);
            if (node?.props?.children) visit(node.props.children);
        };
        visit(picker({open: true, onClose() {}, onSelect() {}}));
        assert.equal(images.length, 1, `${kind} picker hides retired items but keeps them in the registry`);
    }
});
