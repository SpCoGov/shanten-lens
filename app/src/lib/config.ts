import { readTextFile, writeTextFile, exists } from "@tauri-apps/plugin-fs";
import { join } from "@tauri-apps/api/path";
import { dataRoot } from "./dataRoot";

export interface AppConfig {
    language: "zh-CN" | "ja-JP" | "en-US";
    theme: "light" | "dark" | "system";
    backend: { host: string; port: number };
}

export const defaultConfig: AppConfig = {
    language: "zh-CN",
    theme: "system",
    backend: { host: "127.0.0.1", port: 8787 },
};

const LS_KEY = "shanten.config";

async function readFromFs(): Promise<{ cfg: AppConfig; path: string | null }> {
    const root = await dataRoot();
    if (!root) {
        // FS 不可用，走 localStorage
        const raw = localStorage.getItem(LS_KEY);
        if (!raw) {
            localStorage.setItem(LS_KEY, JSON.stringify(defaultConfig));
            return { cfg: defaultConfig, path: null };
        }
        try {
            const obj = JSON.parse(raw);
            return {
                cfg: { ...defaultConfig, ...obj, backend: { ...defaultConfig.backend, ...(obj.backend ?? {}) } },
                path: null,
            };
        } catch {
            localStorage.setItem(LS_KEY, JSON.stringify(defaultConfig));
            return { cfg: defaultConfig, path: null };
        }
    }

    // FS 可用
    const p = await join(root, "config.json");
    const has = await exists(p).catch(() => false);
    if (!has) {
        await writeTextFile(p, JSON.stringify(defaultConfig, null, 2));
        return { cfg: defaultConfig, path: p };
    }
    try {
        const text = await readTextFile(p);
        const obj = JSON.parse(text);
        return {
            cfg: { ...defaultConfig, ...obj, backend: { ...defaultConfig.backend, ...(obj.backend ?? {}) } },
            path: p,
        };
    } catch {
        await writeTextFile(p, JSON.stringify(defaultConfig, null, 2));
        return { cfg: defaultConfig, path: p };
    }
}

async function writeToFs(next: AppConfig): Promise<string | null> {
    const root = await dataRoot();
    if (!root) {
        localStorage.setItem(LS_KEY, JSON.stringify(next));
        return null;
    }
    const p = await join(root, "config.json");
    await writeTextFile(p, JSON.stringify(next, null, 2));
    return p;
}

export async function configPath() {
    const { path } = await readFromFs();
    return path;
}

export async function readConfig(): Promise<AppConfig> {
    const { cfg } = await readFromFs();
    return cfg;
}

export async function writeConfig(next: AppConfig) {
    return await writeToFs(next);
}

