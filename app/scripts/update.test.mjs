import assert from "node:assert/strict";
import {mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync} from "node:fs";
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
        writeFileSync(join(directory, "shanten-lens_3.0.5_windows_x64_portable.zip"), "portable");
        const manifest = createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens");
        assert.equal(manifest.version, "3.0.5");
        assert.equal(manifest.platforms["windows-x86_64"].signature, "signed-package");
        assert.equal(manifest.platforms["windows-x86_64"].url,
            "https://github.com/SpCoGov/shanten-lens/releases/download/v3.0.5/Shanten.Lens_3.0.5_x64-setup.exe");
        assert.throws(() => createUpdateManifest(directory, "3.0.6", "SpCoGov/shanten-lens"));
        writeFileSync(join(directory, "Other_3.0.5_x64-setup.exe"), "ambiguous installer");
        assert.throws(() => createUpdateManifest(directory, "3.0.5", "SpCoGov/shanten-lens"));
    } finally {
        assert.equal(dirname(directory), resolve(tmpdir()));
        rmSync(directory, {recursive: true});
    }
});

test("release manifest finds a signed installer in nested artifacts and rejects duplicates", () => {
    const directory = mkdtempSync(join(tmpdir(), "shanten-update-test-"));
    try {
        const nsis = join(directory, "windows_3.2.0_artifacts", "nsis");
        const portable = join(directory, "windows_3.2.0_artifacts", "portable");
        for (const path of [nsis, portable]) mkdirSync(path, {recursive: true});
        const name = "Shanten Lens_3.2.0_x64-setup.exe";
        writeFileSync(join(nsis, name), "installer");
        writeFileSync(join(portable, "shanten-lens_3.2.0_windows_x64_portable.zip"), "portable");
        writeFileSync(join(directory, "Shanten Lens_3.2.0_aarch64.dmg"), "macOS installer");
        assert.throws(() => createUpdateManifest(directory, "3.2.0", "SpCoGov/shanten-lens"));
        writeFileSync(join(nsis, `${name}.sig`), "nested-signature\n");
        const manifest = createUpdateManifest(directory, "3.2.0", "SpCoGov/shanten-lens");
        assert.equal(manifest.platforms["windows-x86_64"].signature, "nested-signature");
        assert.equal(manifest.platforms["windows-x86_64"].url,
            "https://github.com/SpCoGov/shanten-lens/releases/download/v3.2.0/Shanten.Lens_3.2.0_x64-setup.exe");
        const wrongVersion = join(nsis, "Shanten Lens_3.1.0_x64-setup.exe");
        renameSync(join(nsis, name), wrongVersion);
        writeFileSync(`${wrongVersion}.sig`, "old-signature");
        assert.throws(() => createUpdateManifest(directory, "3.2.0", "SpCoGov/shanten-lens"),
            /Expected exactly one Windows x64 installer/);
        renameSync(wrongVersion, join(nsis, name));
        writeFileSync(join(directory, name), "duplicate installer");
        assert.throws(() => createUpdateManifest(directory, "3.2.0", "SpCoGov/shanten-lens"),
            /Expected exactly one Windows x64 installer/);
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
