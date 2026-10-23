export type TargetRule = {
    faces: string[];
    suits: string[];
    ranks: number[];
    red: boolean;
    dora: boolean;
    soul: boolean;
    joker: "allow" | "exclude" | "only";
};
export const TARGET_KINDS = ["single", "pair", "sequence", "triplet", "meld", "quad"] as const;
export type TargetKind = typeof TARGET_KINDS[number];
export type TargetGroup = {quad: boolean; kind?: TargetKind; suit_group?: string; distinct_group?: string; rule: TargetRule};
export type TargetPreset = {
    format: "shanten-lens.custom-target";
    version: 1;
    name: string;
    target_groups: TargetGroup[];
    wall_limit: number;
    exclude_invalid_shapes: boolean;
};

export const TARGET_PRESETS_KEY = "sl-custom-switch:presets-v1";
export function readTargetPresets(): TargetPreset[] {
    try {
        const saved = JSON.parse(localStorage.getItem(TARGET_PRESETS_KEY) ?? "[]");
        return Array.isArray(saved) ? saved.map(parseTargetPreset).filter((preset): preset is TargetPreset => preset !== null) : [];
    } catch { return []; }
}

export function targetKind(group: Pick<TargetGroup, "quad" | "kind">): TargetKind {
    return group.quad ? "quad" : group.kind ?? "single";
}

export function targetGroupSize(group: TargetGroup) {
    const kind = targetKind(group);
    return kind === "single" ? 1 : kind === "pair" ? 2 : 3;
}

export function newTargetGroup(): TargetGroup {
    return {quad: false, rule: {faces: [], suits: [], ranks: [], red: false, dora: false, soul: false, joker: "allow"}};
}

export function targetSize(groups: TargetGroup[]) {
    return groups.reduce((count, group) => count + targetGroupSize(group), 0);
}

export function applyTargetRule(groups: TargetGroup[], selected: number[], patch: Partial<TargetRule> | ((rule: TargetRule) => Partial<TargetRule>)): TargetGroup[] {
    return groups.map((group, index) => {
        if (!selected.includes(index)) return group;
        const rule = {...group.rule, ...(typeof patch === "function" ? patch(group.rule) : patch)};
        return {...group, rule: {
            ...rule, faces: [...rule.faces], suits: [...rule.suits], ranks: [...rule.ranks],
            joker: targetKind(group) === "quad" ? "exclude" : "allow",
        }};
    });
}

export function restoreTargetGroups(value: unknown): TargetGroup[] {
    if (!validTargetGroups(value)) return Array.from({length: 14}, newTargetGroup);
    // Old per-slot joker preferences must not override the fixed joker in the actual hand.
    return value.map(group => ({...group, quad: targetKind(group) === "quad",
        rule: {...group.rule, joker: targetKind(group) === "quad" ? "exclude" : "allow"}}));
}

export function parseTargetPreset(value: unknown): TargetPreset | null {
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const preset = value as Record<string, unknown>;
    const wallLimit = preset.wall_limit === undefined ? 36 : preset.wall_limit;
    const excludeInvalidShapes = preset.exclude_invalid_shapes === undefined ? false : preset.exclude_invalid_shapes;
    if (preset.format !== "shanten-lens.custom-target" || preset.version !== 1
        || typeof preset.name !== "string" || !preset.name.trim() || preset.name.trim().length > 80
        || !Array.isArray(preset.target_groups) || preset.target_groups.length > 14
        || typeof wallLimit !== "number" || !Number.isInteger(wallLimit) || wallLimit < 0 || wallLimit > 36
        || typeof excludeInvalidShapes !== "boolean") return null;
    const groups = [];
    for (const value of preset.target_groups) {
        if (!value || typeof value !== "object" || Array.isArray(value)) return null;
        const group = value as Record<string, unknown>;
        const rule = group.rule === undefined ? {} : group.rule;
        if (!rule || typeof rule !== "object" || Array.isArray(rule)) return null;
        groups.push({...group, quad: group.quad === undefined ? group.kind === "quad" : group.quad,
            rule: {...newTargetGroup().rule, ...rule}});
    }
    if (!validTargetGroups(groups)) return null;
    return {format: "shanten-lens.custom-target", version: 1, name: preset.name.trim(),
        target_groups: restoreTargetGroups(groups), wall_limit: wallLimit, exclude_invalid_shapes: excludeInvalidShapes};
}

export function compactTargetPreset(preset: TargetPreset) {
    return {format: preset.format, version: preset.version, name: preset.name,
        target_groups: preset.target_groups.map(group => {
            const kind = targetKind(group);
            const defaults = {...newTargetGroup().rule, joker: kind === "quad" ? "exclude" : "allow"};
            const rule = Object.fromEntries((Object.keys(defaults) as (keyof TargetRule)[])
                .filter(key => Array.isArray(group.rule[key]) ? group.rule[key].length > 0 : group.rule[key] !== defaults[key])
                .map(key => [key, group.rule[key]]));
            return {...(kind !== "single" ? {kind} : {}),
                ...(group.suit_group?.trim() ? {suit_group: group.suit_group.trim()} : {}),
                ...(group.distinct_group?.trim() ? {distinct_group: group.distinct_group.trim()} : {}),
                ...(Object.keys(rule).length ? {rule} : {})};
        }),
        ...(preset.wall_limit !== 36 ? {wall_limit: preset.wall_limit} : {}),
        ...(preset.exclude_invalid_shapes ? {exclude_invalid_shapes: true} : {})};
}

export function validTargetGroups(value: unknown): value is TargetGroup[] {
    if (!Array.isArray(value) || value.length > 14) return false;
    return value.every(group => {
        const rule = group?.rule;
        return typeof group?.quad === "boolean" && rule
            && (group.kind === undefined || TARGET_KINDS.includes(group.kind))
            && (!group.quad || group.kind === undefined || group.kind === "single" || group.kind === "quad")
            && [group.suit_group, group.distinct_group].every(link => link === undefined || (typeof link === "string" && link.length <= 32))
            && Array.isArray(rule.faces) && rule.faces.length <= 37 && rule.faces.every((v: unknown) => typeof v === "string" && /^(?:[0-9][mps]|[1-7]z)$/.test(v))
            && Array.isArray(rule.suits) && rule.suits.length <= 4 && rule.suits.every((v: unknown) => typeof v === "string" && ["m", "p", "s", "z"].includes(v))
            && Array.isArray(rule.ranks) && rule.ranks.length <= 9 && rule.ranks.every((v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 1 && v <= 9)
            && ["red", "dora", "soul"].every(key => typeof rule[key] === "boolean")
            && ["allow", "exclude", "only"].includes(rule.joker);
    });
}
