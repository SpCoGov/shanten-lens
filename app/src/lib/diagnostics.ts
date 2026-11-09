import {APP_VERSION} from "./version";
import {useLogStore} from "./logStore";

// Only UI preferences belong here; never enumerate localStorage or include cached account data.
export const DIAGNOSTIC_SETTINGS_KEYS = [
    "sl-lang", "sl-theme", "sl-custom-theme", "sl-tile-skin",
    "sl-hud:enabled", "sl-hud:show-blackhole", "sl-hud:show-wanxiang",
    "sl-hud:show-custom-switch", "sl-hud:show-score-projection", "sl-hud:show-current-level",
    "sl-custom-switch:target-v1", "sl-custom-switch:wall-limit", "sl-custom-switch:exclude-invalid-shapes",
    "sl-custom-switch:presets-v1", "sl-wanxiang-switch:prefer-bonus", "sl-wanxiang-switch:preferred-suit",
    "sl-wanxiang-switch:preferred-meld-type", "sl-wanxiang-switch:coin-objective",
] as const;

export function collectFrontendDiagnostics() {
    const settings: Record<string, string | null> = {};
    let storageError: string | undefined;
    try {
        for (const key of DIAGNOSTIC_SETTINGS_KEYS) settings[key] = localStorage.getItem(key);
    } catch (error) { storageError = String(error); }
    return {
        app_version: APP_VERSION,
        exported_at: new Date().toISOString(),
        time_zone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        utc_offset_minutes: -new Date().getTimezoneOffset(),
        user_agent: navigator.userAgent,
        language: navigator.language,
        display: {width: window.innerWidth, height: window.innerHeight, scale: window.devicePixelRatio},
        settings,
        storage_error: storageError ?? null,
        logs: useLogStore.getState().logs.filter((entry) => entry.target === "shanten_lens::frontend")
            .map((entry) => ({ts_ms: entry.ts_ms, level: entry.level, target: entry.target, message: entry.msg})),
    };
}
