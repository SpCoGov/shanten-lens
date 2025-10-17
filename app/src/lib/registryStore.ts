import { useSyncExternalStore } from "react";

export type Amulet = { id: number; icon_id: number; name: string; rarity: "GREEN"|"BLUE"|"ORANGE"|"PURPLE" };
export type Badge  = { id: number; icon_id: number; name: string; rarity: "BROWN"|"BLUE"|"RED" };

export type RegistryPayload = {
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

const subs = new Set<() => void>();
const emit = () => subs.forEach(fn => fn());

function buildMaps(next: RegistryPayload): State {
    const amuletById = new Map<number, Amulet>();
    const badgeById  = new Map<number, Badge>();
    for (const a of next.amulets) amuletById.set(a.id, a);
    for (const b of next.badges)  badgeById.set(b.id, b);
    return { amulets: next.amulets, badges: next.badges, amuletById, badgeById };
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
if (cached) state = buildMaps(cached);

export function setRegistry(payload: RegistryPayload) {
    state = buildMaps(payload);
    saveToLS(payload);
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