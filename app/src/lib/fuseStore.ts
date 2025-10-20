import {useSyncExternalStore} from "react";

export type FuseConfig = {
    guard_skip_contains: {
        amulets: number[];
        badges: number[];
    };
    enable_prestart_kavi_guard?: boolean;
    conduction_min_count?: number;
    enable_anti_steal_eat?: boolean;
    enable_kavi_plus_buffer_guard?: boolean;
};

const defaultConfig: FuseConfig = {
    guard_skip_contains: {amulets: [], badges: []},
};

type State = {
    config: FuseConfig;
    selected: { amulets: Set<number>; badges: Set<number> };
};

let state: State = {
    config: defaultConfig,
    selected: {amulets: new Set(), badges: new Set()},
};

const subs = new Set<() => void>();
const emit = () => subs.forEach((fn) => fn());

export function setFuseConfig(cfg: FuseConfig) {
    state = {...state, config: cfg ?? defaultConfig};
    emit();
}

export function patchFuseConfig(patch: Partial<FuseConfig>) {
    state = {...state, config: {...state.config, ...patch} as FuseConfig};
    emit();
}

export function toggleSelect(kind: "amulet" | "badge", id: number) {
    const a = new Set(state.selected.amulets);
    const b = new Set(state.selected.badges);
    if (kind === "amulet") {
        a.has(id) ? a.delete(id) : a.add(id);
    } else {
        b.has(id) ? b.delete(id) : b.add(id);
    }
    state = {...state, selected: {amulets: a, badges: b}}; // 新引用
    emit();
}

export function clearSelection() {
    state = {...state, selected: {amulets: new Set(), badges: new Set()}}; // 新引用
    emit();
}

export function removeSelected() {
    const a = state.config.guard_skip_contains.amulets.filter(
        (id) => !state.selected.amulets.has(id)
    );
    const b = state.config.guard_skip_contains.badges.filter(
        (id) => !state.selected.badges.has(id)
    );
    state = {
        config: {guard_skip_contains: {amulets: a, badges: b}},
        selected: {amulets: new Set(), badges: new Set()},
    };
    emit();
}

export function addAmulet(id: number) {
    const arr = state.config.guard_skip_contains.amulets;
    if (!arr.includes(id)) {
        state = {
            ...state,
            config: {
                ...state.config,
                guard_skip_contains: {...state.config.guard_skip_contains, amulets: [...arr, id]},
            },
        };
        emit();
    }
}

export function addBadge(id: number) {
    const arr = state.config.guard_skip_contains.badges;
    if (!arr.includes(id)) {
        state = {
            ...state,
            config: {
                ...state.config,
                guard_skip_contains: {...state.config.guard_skip_contains, badges: [...arr, id]},
            },
        };
        emit();
    }
}

export function useFuse() {
    return useSyncExternalStore(
        (fn) => {
            subs.add(fn);
            return () => subs.delete(fn);
        },
        () => state
    );
}