import type {SearchPreferences} from "./planTypes";

export const WANXIANG_PREFER_BONUS_KEY = "sl-wanxiang-switch:prefer-bonus";
export const WANXIANG_PREFERRED_SUIT_KEY = "sl-wanxiang-switch:preferred-suit";
export const WANXIANG_PREFERRED_MELD_TYPE_KEY = "sl-wanxiang-switch:preferred-meld-type";
export const WANXIANG_COIN_OBJECTIVE_KEY = "sl-wanxiang-switch:coin-objective";

export function updateWanxiangPreferences(preferences: SearchPreferences, patch: Partial<SearchPreferences>): SearchPreferences {
    const next = {...preferences, coin_objective: null, ...patch};
    return next.coin_objective ? {
        ...next, prefer_dora: false, prefer_soul: false, any_waits: false,
        preferred_suit: null, preferred_meld_type: null,
    } : next;
}

export function readWanxiangPreferences(): SearchPreferences {
    const preferences: SearchPreferences = {
        prefer_dora: false, prefer_soul: false, any_waits: false,
        preferred_suit: null, preferred_meld_type: null, coin_objective: null,
    };
    try {
        preferences.prefer_dora = preferences.prefer_soul = localStorage.getItem(WANXIANG_PREFER_BONUS_KEY) === "1";
        const suit = localStorage.getItem(WANXIANG_PREFERRED_SUIT_KEY);
        const meldType = localStorage.getItem(WANXIANG_PREFERRED_MELD_TYPE_KEY);
        preferences.preferred_suit = (["z", "p", "s", "m"] as const).find(value => value === suit) ?? null;
        preferences.preferred_meld_type = (["triplet", "sequence"] as const).find(value => value === meldType) ?? null;
        const objective = localStorage.getItem(WANXIANG_COIN_OBJECTIVE_KEY);
        return updateWanxiangPreferences(preferences, {
            coin_objective: (["heavenly_blessing", "shining_pearl"] as const).find(value => value === objective) ?? null,
        });
    } catch {
    }
    return preferences;
}
