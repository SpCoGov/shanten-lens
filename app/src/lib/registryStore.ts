import { useSyncExternalStore } from "react";
import i18n from "i18next";
import type {GameStateData} from "./gamestate";
import type {RegistryMetadata, RegistryTranslations} from "../bindings";

export type Amulet = {
    id: number; icon_id: number; name: string; rarity: "GREEN"|"BLUE"|"ORANGE"|"PURPLE"|"GRAY";
    plus_name?: string; sell_price?: number; plus_sell_price?: number;
    metadata?: RegistryMetadata | null; plus_metadata?: RegistryMetadata | null;
    names?: RegistryTranslations | null; plus_names?: RegistryTranslations | null;
};
export type Badge  = { id: number; icon_id: number; name: string; rarity: "BROWN"|"BLUE"|"RED"; metadata?: RegistryMetadata | null };

export type RegistryPayload = {
    source?: GameStateData["source"];
    amulets: Amulet[];
    badges:  Badge[];
};

type State = {
    amulets: Amulet[];
    badges: Badge[];
    amuletById: Map<number, Amulet>;
    badgeById: Map<number, Badge>;
};

const LS_KEY = "shanten:registry:v1";

let state: State = {
    amulets: [],
    badges: [],
    amuletById: new Map(),
    badgeById: new Map(),
};
let payload: RegistryPayload = { amulets: [], badges: [] };

const subs = new Set<() => void>();
const emit = () => subs.forEach(fn => fn());

function buildMaps(next: RegistryPayload): State {
    const language = (i18n.resolvedLanguage || i18n.language || "zh-CN").replace(/_/g, "-").toLowerCase();
    const key: keyof RegistryMetadata["names"] = language.startsWith("zh")
        ? /^zh-(?:tw|hk|mo|hant)(?:-|$)/.test(language) ? "chs_t" : "chs"
        : language.startsWith("ja") ? "jp" : language.startsWith("ko") ? "kr" : language.startsWith("en") ? "en" : "chs";
    const name = (names: RegistryTranslations | null | undefined, fallback: string) => names?.[key]?.trim() || fallback;
    const amulets = next.amulets.map((amulet) => ({
        ...amulet,
        name: name(amulet.names ?? amulet.metadata?.names, amulet.name),
        plus_name: name(amulet.plus_names ?? amulet.plus_metadata?.names, amulet.plus_name ?? "") || undefined,
    }));
    const badges = next.badges.map((badge) => ({ ...badge, name: name(badge.metadata?.names, badge.name) }));
    const amuletById = new Map<number, Amulet>();
    const badgeById  = new Map<number, Badge>();
    for (const a of amulets) amuletById.set(a.id, a);
    for (const b of badges)  badgeById.set(b.id, b);
    return { amulets, badges, amuletById, badgeById };
}

function saveToLS(next: RegistryPayload) {
    try { localStorage.setItem(LS_KEY, JSON.stringify(next)); } catch {}
}
function loadFromLS(): RegistryPayload | null {
    try {
        const txt = localStorage.getItem(LS_KEY);
        if (!txt) return null;
        const obj = JSON.parse(txt);
        if (!obj || !Array.isArray(obj.amulets) || !Array.isArray(obj.badges)) return null;
        return obj as RegistryPayload;
    } catch { return null; }
}

const cached = loadFromLS();
if (cached) {
    payload = cached;
    state = buildMaps(payload);
}

i18n.on("languageChanged", () => {
    state = buildMaps(payload);
    emit();
});

export function setRegistry(next: RegistryPayload) {
    payload = next;
    state = buildMaps(next);
    if (next.source !== "qyzz") saveToLS(next);
    emit();
}

export function getRegistry() { return state; }
export function getAmulet(id: number) { return state.amuletById.get(id) || null; }
export function getBadge(id: number)  { return state.badgeById.get(id)  || null; }

export function useRegistry() {
    return useSyncExternalStore(
        (fn) => { subs.add(fn); return () => subs.delete(fn); },
        () => state
    );
}

export function useAmulets() {
    return useSyncExternalStore(
        (fn) => { subs.add(fn); return () => subs.delete(fn); },
        () => state.amulets
    );
}
export function useBadges() {
    return useSyncExternalStore(
        (fn) => { subs.add(fn); return () => subs.delete(fn); },
        () => state.badges
    );
}
