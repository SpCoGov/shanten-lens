export const HUD_ENABLED_KEY = "sl-hud:enabled";
export const HUD_SHOW_BLACKHOLE_KEY = "sl-hud:show-blackhole";
export const HUD_SHOW_SCORE_PROJECTION_KEY = "sl-hud:show-score-projection";

export function readHudEnabled() {
    try {
        return localStorage.getItem(HUD_ENABLED_KEY) !== "0";
    } catch {
        return true;
    }
}

export function writeHudEnabled(value: boolean) {
    try {
        localStorage.setItem(HUD_ENABLED_KEY, value ? "1" : "0");
    } catch {
    }
}

export function readHudShowBlackhole() {
    try {
        return localStorage.getItem(HUD_SHOW_BLACKHOLE_KEY) === "1";
    } catch {
        return false;
    }
}

export function writeHudShowBlackhole(value: boolean) {
    try {
        localStorage.setItem(HUD_SHOW_BLACKHOLE_KEY, value ? "1" : "0");
    } catch {
    }
}

export function readHudShowScoreProjection() {
    try {
        return localStorage.getItem(HUD_SHOW_SCORE_PROJECTION_KEY) === "1";
    } catch {
        return false;
    }
}

export function writeHudShowScoreProjection(value: boolean) {
    try {
        localStorage.setItem(HUD_SHOW_SCORE_PROJECTION_KEY, value ? "1" : "0");
    } catch {
    }
}
