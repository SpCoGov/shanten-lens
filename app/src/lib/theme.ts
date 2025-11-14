export type ThemeMode = "auto" | "dark" | "dark-green";
const THEME_KEY = "sl-theme";

export function readTheme(): ThemeMode {
    const saved = localStorage.getItem(THEME_KEY) as ThemeMode | null;
    if (saved === "auto" || saved === "dark" || saved === "dark-green") return saved;
    return "auto";
}

export function applyTheme(mode: ThemeMode) {
    const root = document.documentElement;
    root.removeAttribute("data-theme");
    if (mode === "auto") {
        const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
        if (prefersDark) root.setAttribute("data-theme", "dark");
        return;
    }
    root.setAttribute("data-theme", mode);
}