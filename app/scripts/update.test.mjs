import assert from "node:assert/strict";
import {mkdtempSync, readFileSync, rmSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {dirname, join, resolve} from "node:path";
import {test} from "node:test";
import {runInNewContext} from "node:vm";
import ts from "typescript";
import {createUpdateManifest} from "../../scripts/update-manifest.mjs";

test("release manifest requires one matching installer and its signature", () => {
    const directory = mkdtempSync(join(tmpdir(), "shanten-update-test-"));
    try {
        const name = "Shanten Lens_3.0.5_x64-setup.exe";
        assert.throws(() => createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens"));
        writeFileSync(join(directory, name), "installer");
        assert.throws(() => createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens"));
        writeFileSync(join(directory, `${name}.sig`), "signed-package\n");
        const manifest = createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens");
        assert.equal(manifest.version, "3.0.5");
        assert.equal(manifest.platforms["windows-x86_64"].signature, "signed-package");
        assert.equal(manifest.platforms["windows-x86_64"].url,
            "https://github.com/SpCoGov/shanten-lens/releases/download/v3.0.5/Shanten%20Lens_3.0.5_x64-setup.exe");
        assert.throws(() => createUpdateManifest(directory, "3.0.6", "SpCoGov/shanten-lens"));
        writeFileSync(join(directory, "Other_3.0.5_x64-setup.exe"), "ambiguous installer");
        assert.throws(() => createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens"));
    } finally {
        assert.equal(dirname(directory), resolve(tmpdir()));
        rmSync(directory, {recursive: true});
    }
});

test("updates honor preferences, select NSIS, and propagate verification failures", async () => {
    const stored = new Map(), calls = [];
    let target = "windows", enabled = true, downloadError = null;
    const assets = ["old.msi", "portable.zip", "Shanten Lens_3.0.5_x64-setup.exe", "Shanten Lens.dmg"]
        .map((name) => ({name, browser_download_url: `https://example.com/${name}`}));
    const mocks = {
        "./version": {APP_VERSION: "3.0.4"},
        "@tauri-apps/plugin-os": {platform: () => target},
        "@tauri-apps/api/core": {
            Channel: class {},
            invoke: async (name, args) => {
                calls.push({name, args});
                if (name === "fetch_latest_release") return JSON.stringify({tag_name: "v3.0.5", assets});
                if (name === "app_updater_enabled") return enabled;
                if (name === "download_app_update") {
                    args.onProgress.onmessage({downloaded: 50, total: 100});
                    if (downloadError) throw downloadError;
                    return;
                }
                assert.equal(name, "discard_app_update");
            },
        },
    };
    const exports = {};
    const code = ts.transpileModule(readFileSync(new URL("../src/lib/updateCheck.ts", import.meta.url), "utf8"), {
        compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020},
    }).outputText;
    runInNewContext(code, {
        exports, require: (name) => mocks[name],
        localStorage: {getItem: (key) => stored.get(key), setItem: (key, value) => stored.set(key, value)},
    });
    exports.setUpdateAutoCheck(false);
    assert.equal((await exports.checkForUpdates()).status, "disabled");
    assert.equal(calls.length, 0);
    const {update} = await exports.checkForUpdates({manual: true});
    assert.equal(update.downloadAssetName, assets[2].name);
    assert.equal(update.canInstall, true);
    exports.ignoreUpdateVersion("3.0.5");
    exports.setUpdateAutoCheck(true);
    assert.equal((await exports.checkForUpdates()).status, "ignored");
    assert.equal((await exports.checkForUpdates({manual: true})).status, "available");
    exports.setUpdateUseSystemProxy(false);
    downloadError = new Error("signature verification failed");
    const progress = [];
    await assert.rejects(exports.downloadUpdate(update, (value) => progress.push(value)), /signature verification/);
    assert.equal(progress[0].downloaded, 50);
    const download = calls.at(-1);
    assert.equal(download.args.version, "3.0.5");
    assert.equal(download.args.useSystemProxy, false);
    assert.equal(calls.some(({name}) => /install|shutdown/.test(name)), false, "Downloading must never install or exit");
    await exports.discardUpdate();
    assert.equal(calls.at(-1).name, "discard_app_update");
    target = "macos";
    enabled = false;
    const mac = (await exports.checkForUpdates({manual: true})).update;
    assert.equal(mac.downloadAssetName, assets[3].name);
    assert.equal(mac.canInstall, false);
});
