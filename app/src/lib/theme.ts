export type ThemeMode = "auto" | "dark" | "dark-green" | "dark-purple" | "custom";
const THEME_KEY = "sl-theme";
const CUSTOM_THEME_KEY = "sl-custom-theme";

export const CUSTOM_THEME_COLORS = [
    {key: "--color-bg", label: "background"},
    {key: "--panel-bg", label: "panel"},
    {key: "--color-text", label: "text"},
    {key: "--color-muted", label: "muted"},
    {key: "--color-border", label: "border"},
    {key: "--color-ring", label: "accent"},
    {key: "--input-bg", label: "input"},
    {key: "--state-ok", label: "success"},
    {key: "--state-warn", label: "warning"},
    {key: "--state-down", label: "danger"},
] as const;
export type CustomThemeColor = typeof CUSTOM_THEME_COLORS[number]["key"];
export type CustomTheme = {
    base: "light" | "dark";
    colors: Partial<Record<CustomThemeColor, string>>;
};

export function createRandomTheme(base: CustomTheme["base"]): CustomTheme {
    const hue = Math.random() * 360;
    const dark = base === "dark";
    const color = (h: number, saturation: number, lightness: number) => {
        const light = lightness / 100;
        const chroma = saturation / 100 * Math.min(light, 1 - light);
        // Convert HSL to the hex format used by the color inputs and storage.
        return "#" + [0, 8, 4].map((offset) => {
            const k = (offset + h / 30) % 12;
            const channel = light - chroma * Math.max(-1, Math.min(k - 3, 9 - k, 1));
            return Math.round(channel * 255).toString(16).padStart(2, "0");
        }).join("");
    };
    return {
        base,
        colors: {
            "--color-bg": color(hue, 24, dark ? 7 : 97),
            "--panel-bg": color(hue, 24, dark ? 11 : 99),
            "--color-text": color(hue, 15, dark ? 93 : 12),
            "--color-muted": color(hue, 12, dark ? 70 : 37),
            "--color-border": color(hue, 24, dark ? 25 : 78),
            "--color-ring": color(hue, 60, dark ? 72 : 26),
            "--input-bg": color(hue, 24, dark ? 9 : 95),
            "--state-ok": color(130 + Math.random() * 30, 60, dark ? 65 : 30),
            "--state-warn": color(30 + Math.random() * 15, 80, dark ? 65 : 30),
            "--state-down": color(Math.random() * 10, 70, dark ? 72 : 35),
        },
    };
}

function normalizeCustomTheme(value: unknown): CustomTheme {
    const config = value && typeof value === "object" ? value as Partial<CustomTheme> : {};
    const colors: CustomTheme["colors"] = {};
    for (const {key} of CUSTOM_THEME_COLORS) {
        const color = config.colors?.[key];
        if (typeof color === "string" && /^#[\da-f]{6}$/i.test(color)) colors[key] = color;
    }
    return {base: config.base === "light" ? "light" : "dark", colors};
}

export function readCustomTheme(): CustomTheme {
    try {
        return normalizeCustomTheme(JSON.parse(localStorage.getItem(CUSTOM_THEME_KEY) ?? "null"));
    } catch {
        return normalizeCustomTheme(null);
    }
}

export function readTheme(): ThemeMode {
    try {
        const saved = localStorage.getItem(THEME_KEY);
        if (saved === "auto" || saved === "dark" || saved === "dark-green" || saved === "dark-purple" || saved === "custom") return saved;
    } catch {
    }
    return "auto";
}

export function applyTheme(mode: ThemeMode) {
    const root = document.documentElement;
    for (const {key} of CUSTOM_THEME_COLORS) root.style.removeProperty(key);
    root.removeAttribute("data-custom-theme");
    root.removeAttribute("data-theme");
    if (mode === "custom") {
        const custom = readCustomTheme();
        if (custom.base === "dark") root.setAttribute("data-theme", "dark");
        root.setAttribute("data-custom-theme", custom.base);
        for (const [key, color] of Object.entries(custom.colors)) root.style.setProperty(key, color);
    } else if (mode === "auto") {
        const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
        if (prefersDark) root.setAttribute("data-theme", "dark");
    } else {
        root.setAttribute("data-theme", mode);
    }
}

export function setTheme(mode: ThemeMode) {
    localStorage.setItem(THEME_KEY, mode);
    applyTheme(mode);
    window.dispatchEvent(new Event("sl:theme-change"));
}

export function setCustomTheme(custom: CustomTheme) {
    localStorage.setItem(CUSTOM_THEME_KEY, JSON.stringify(normalizeCustomTheme(custom)));
    applyTheme(readTheme());
    window.dispatchEvent(new Event("sl:theme-change"));
}

export function subscribeThemeChanges(callback: () => void) {
    const onStorage = (event: StorageEvent) => {
        if (event.key === null || event.key === THEME_KEY || event.key === CUSTOM_THEME_KEY) callback();
    };
    window.addEventListener("storage", onStorage);
    window.addEventListener("sl:theme-change", callback);
    return () => {
        window.removeEventListener("storage", onStorage);
        window.removeEventListener("sl:theme-change", callback);
    };
}

export function initializeTheme() {
    const sync = () => applyTheme(readTheme());
    sync();
    const unsubscribe = subscribeThemeChanges(sync);
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onMedia = () => { if (readTheme() === "auto") sync(); };
    media?.addEventListener("change", onMedia);
    return () => {
        unsubscribe();
        media?.removeEventListener("change", onMedia);
    };
}
