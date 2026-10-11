import type {SearchPreferences} from "./planTypes";

export const WANXIANG_PREFER_BONUS_KEY = "sl-wanxiang-switch:prefer-bonus";
export const WANXIANG_PREFERRED_SUIT_KEY = "sl-wanxiang-switch:preferred-suit";
export const WANXIANG_PREFERRED_MELD_TYPE_KEY = "sl-wanxiang-switch:preferred-meld-type";

export function readWanxiangPreferences(): SearchPreferences {
    const preferences: SearchPreferences = {
        prefer_dora: false, prefer_soul: false, any_waits: false,
        preferred_suit: null, preferred_meld_type: null,
    };
    try {
        preferences.prefer_dora = preferences.prefer_soul = localStorage.getItem(WANXIANG_PREFER_BONUS_KEY) === "1";
        const suit = localStorage.getItem(WANXIANG_PREFERRED_SUIT_KEY);
        const meldType = localStorage.getItem(WANXIANG_PREFERRED_MELD_TYPE_KEY);
        preferences.preferred_suit = (["z", "p", "s", "m"] as const).find(value => value === suit) ?? null;
        preferences.preferred_meld_type = (["triplet", "sequence"] as const).find(value => value === meldType) ?? null;
    } catch {
    }
    return preferences;
}
