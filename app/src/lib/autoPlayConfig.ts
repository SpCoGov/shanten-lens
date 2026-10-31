import {newTargetGroup, targetKind, targetSize, validTargetGroups, type TargetGroup, type TargetPreset} from "./customTarget";
import {getGameLevels} from "./gameStages";
import type {GameStateData} from "./gamestate";
import qyzzStages from "./qyzzStageCatalog.json";
import type {Amulet, Badge} from "./registryStore";
import type {TargetItem} from "./autoRunnerStore";

export type StrategyAmulet = {key: string; id: number; plus: boolean; badge: number | null};
export type StrategyLoadout = {
    id: string;
    name: string;
    amulets: StrategyAmulet[];
    entry_order: string[];
    win_order: string[];
};
export type StrategyHand = {id: string; name: string; target_groups: TargetGroup[]; exclude_invalid_shapes: boolean};
export type TacticalStage = {
    id: string;
    name: string;
    start_level: number;
    start_node: number;
    deadline_level: number;
    deadline_node: number;
    enter_early_if_unreachable: boolean;
    loadouts: StrategyLoadout[];
    hands: StrategyHand[];
    hand_selection: "most_wins" | "priority";
};
export type FullPlayProfile = {
    id: string;
    name: string;
    strategy?: "full"; // Profiles saved before strategy selection are full runs.
    character_id: number;
    initial_amulet_id: number;
    first_level_min_coin: string;
    stages: TacticalStage[];
};
export type OpeningPlayProfile = {
    id: string;
    name: string;
    strategy: "opening";
    end_count: number;
    targets: TargetItem[];
    cutoff_level: number;
    cutoff_node: number;
};
export type PlayProfile = FullPlayProfile | OpeningPlayProfile;
export type AutoPlayConfig = {version: 1; selected_profile_id: string; profiles: PlayProfile[]};
export const emptyAutoPlayConfig: AutoPlayConfig = {version: 1, selected_profile_id: "", profiles: []};
export const openingLevels = getGameLevels().flatMap(level => level.nodes.filter(node => node.type !== 1)
    .map(node => ({level: level.level, node: node.node, label: node.label})));
export const strategyLevels = openingLevels.slice(1);

const qyzzOpeningLevels = qyzzStages.levels.map(stage => ({level: stage.id, node: 0, label: stage.name}));

export function getOpeningLevels(source?: GameStateData["source"]) {
    return source === "qyzz" ? qyzzOpeningLevels : openingLevels;
}

export function getStrategyLevels(source?: GameStateData["source"]) {
    return source === "qyzz" ? qyzzOpeningLevels.slice(1) : strategyLevels;
}

export function newStrategyLoadout(name: string): StrategyLoadout {
    return {id: crypto.randomUUID(), name, amulets: [], entry_order: [], win_order: []};
}

export function newStrategyHand(name: string): StrategyHand {
    return {id: crypto.randomUUID(), name, target_groups: Array.from({length: 14}, newTargetGroup), exclude_invalid_shapes: true};
}

export function handFromTargetPreset(preset: TargetPreset): StrategyHand {
    return {id: crypto.randomUUID(), name: preset.name, target_groups: structuredClone(preset.target_groups),
        exclude_invalid_shapes: preset.exclude_invalid_shapes};
}

export function newTacticalStage(name: string, loadoutName: string, handName: string, level = strategyLevels[0]): TacticalStage {
    return {id: crypto.randomUUID(), name, start_level: level.level, start_node: level.node, deadline_level: level.level, deadline_node: level.node,
        enter_early_if_unreachable: false, loadouts: [newStrategyLoadout(loadoutName)],
        hands: [newStrategyHand(handName)], hand_selection: "most_wins"};
}

export function moveItem<T>(items: T[], index: number, offset: number): T[] {
    const next = [...items];
    const destination = index + offset;
    if (index < 0 || index >= items.length || destination < 0 || destination >= items.length) return next;
    next.splice(destination, 0, next.splice(index, 1)[0]);
    return next;
}

export function removeStrategyAmulet(loadout: StrategyLoadout, key: string): StrategyLoadout {
    return {...loadout, amulets: loadout.amulets.filter(item => item.key !== key),
        entry_order: loadout.entry_order.filter(item => item !== key), win_order: loadout.win_order.filter(item => item !== key)};
}

// Validate persisted/imported data before it reaches the editors; never silently replace a broken strategy.
export function validateAutoPlayConfig(value: unknown, amulets?: Map<number, Amulet>, badges?: Map<number, Badge>): string | null {
    const object = (v: unknown): v is Record<string, any> => !!v && typeof v === "object" && !Array.isArray(v);
    const text = (v: unknown) => typeof v === "string" && !!v.trim() && v.length <= 80;
    const list = (v: unknown, min: number, max: number): v is any[] => Array.isArray(v) && v.length >= min && v.length <= max;
    const unique = (items: any[], field: string) => items.every(item => object(item) && text(item[field]))
        && new Set(items.map(item => item[field])).size === items.length;
    const positiveInteger = (v: unknown) => Number.isSafeInteger(v) && Number(v) > 0;
    if (!object(value) || value.version !== 1 || !list(value.profiles, 0, 32) || !unique(value.profiles, "id")
        || typeof value.selected_profile_id !== "string"
        || (value.profiles.length ? !value.profiles.some(p => p.id === value.selected_profile_id) : value.selected_profile_id !== "")) return "format";
    for (const profile of value.profiles) {
        const node = profile.strategy === "opening" ? profile.cutoff_node : profile.stages?.[0]?.start_node;
        const source = node === 0 ? "qyzz" : "packet";
        const openingLevels = getOpeningLevels(source), strategyLevels = getStrategyLevels(source);
        const levelIndex = (level: unknown, node: unknown) => strategyLevels.findIndex(item => item.level === level && item.node === node);
        if (!text(profile.name)) return "profile";
        if (profile.strategy === "opening") {
            if (!positiveInteger(profile.end_count)) return "goal_value";
            if (!openingLevels.some(item => item.level === profile.cutoff_level && item.node === profile.cutoff_node)) return "cutoff";
            if (!list(profile.targets, 1, 128)) return "targets";
            for (const target of profile.targets) {
                if (!object(target) || !positiveInteger(target.id) || (target.value !== undefined && !positiveInteger(target.value))) return "targets";
                if (target.kind === "amulet") {
                    if ((amulets && !amulets.has(target.id)) || (target.plus !== undefined && typeof target.plus !== "boolean")
                        || (target.badge != null && (!positiveInteger(target.badge) || (badges && !badges.has(target.badge))))) return "targets";
                } else if (target.kind !== "badge" || (badges && !badges.has(target.id))) return "targets";
            }
            continue;
        }
        if (profile.strategy !== undefined && profile.strategy !== "full") return "strategy";
        if (![201, 202, 203, 204].includes(profile.character_id)) return "profile";
        if (!Number.isSafeInteger(profile.initial_amulet_id) || profile.initial_amulet_id <= 0) return "initial";
        const initial = amulets?.get(profile.initial_amulet_id);
        if (amulets && (!initial || initial.metadata?.deprecated || !["GREEN", "BLUE"].includes(initial.rarity))) return "initial";
        if (typeof profile.first_level_min_coin !== "string" || !/^(0|[1-9][0-9]{0,127})$/.test(profile.first_level_min_coin)) return "coin";
        if (!list(profile.stages, 1, 32) || !unique(profile.stages, "id")) return "stages";
        let previousStart = -1;
        for (const stage of profile.stages) {
            const start = levelIndex(stage.start_level, stage.start_node);
            if (!text(stage.name) || start < 0 || levelIndex(stage.deadline_level, stage.deadline_node) < 0
                || start <= previousStart
                || typeof stage.enter_early_if_unreachable !== "boolean") return "levels";
            previousStart = start;
            if (!list(stage.loadouts, 1, 16) || !unique(stage.loadouts, "id")) return "loadouts";
            for (const loadout of stage.loadouts) {
                if (!text(loadout.name) || !list(loadout.amulets, 1, 32) || !unique(loadout.amulets, "key")) return "loadouts";
                for (const item of loadout.amulets) {
                    if (!Number.isSafeInteger(item.id) || item.id <= 0 || typeof item.plus !== "boolean"
                        || (item.badge !== null && (!Number.isSafeInteger(item.badge) || item.badge < 0))) return "amulet";
                    if (amulets && !amulets.has(item.id)) return "amulet";
                    if (badges && item.badge && !badges.has(item.badge)) return "amulet";
                }
                const keys = new Set(loadout.amulets.map((item: StrategyAmulet) => item.key));
                if (![loadout.entry_order, loadout.win_order].every(order => Array.isArray(order) && order.length === keys.size
                    && new Set(order).size === keys.size && order.every(key => keys.has(key)))) return "order";
            }
            if (!["most_wins", "priority"].includes(stage.hand_selection)
                || !list(stage.hands, 1, 32) || !unique(stage.hands, "id")) return "hands";
            for (const hand of stage.hands) {
                if (!text(hand.name) || typeof hand.exclude_invalid_shapes !== "boolean" || !validTargetGroups(hand.target_groups)
                    || targetSize(hand.target_groups) !== 14 || hand.target_groups.filter((g: TargetGroup) => targetKind(g) === "quad").length > 4) return "hands";
            }
        }
    }
    return null;
}
