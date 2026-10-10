export type TargetRule = {
    faces: string[];
    suits: string[];
    ranks: number[];
    red: boolean;
    dora: boolean;
    soul: boolean;
    joker: "allow" | "exclude" | "only";
};
export type TargetGroup = {quad: boolean; rule: TargetRule};

export function newTargetGroup(): TargetGroup {
    return {quad: false, rule: {faces: [], suits: [], ranks: [], red: false, dora: false, soul: false, joker: "allow"}};
}

export function targetSize(groups: TargetGroup[]) {
    return groups.reduce((count, group) => count + (group.quad ? 3 : 1), 0);
}

export function applyTargetRule(groups: TargetGroup[], selected: number[], rule: TargetRule): TargetGroup[] {
    return groups.map((group, index) => selected.includes(index) ? {...group, rule: {
        ...rule, faces: [...rule.faces], suits: [...rule.suits], ranks: [...rule.ranks],
        joker: group.quad ? "exclude" : "allow",
    }} : group);
}

export function restoreTargetGroups(value: unknown): TargetGroup[] {
    if (!validTargetGroups(value)) return Array.from({length: 14}, newTargetGroup);
    // Old per-slot joker preferences must not override the fixed joker in the actual hand.
    return value.map(group => ({...group, rule: {...group.rule, joker: group.quad ? "exclude" : "allow"}}));
}

export function validTargetGroups(value: unknown): value is TargetGroup[] {
    if (!Array.isArray(value) || value.length > 14) return false;
    return value.every(group => {
        const rule = group?.rule;
        return typeof group?.quad === "boolean" && rule
            && Array.isArray(rule.faces) && rule.faces.length <= 37 && rule.faces.every((v: unknown) => typeof v === "string" && /^(?:[0-9][mps]|[1-7]z)$/.test(v))
            && Array.isArray(rule.suits) && rule.suits.length <= 4 && rule.suits.every((v: unknown) => typeof v === "string" && ["m", "p", "s", "z"].includes(v))
            && Array.isArray(rule.ranks) && rule.ranks.length <= 9 && rule.ranks.every((v: unknown) => typeof v === "number" && Number.isInteger(v) && v >= 1 && v <= 9)
            && ["red", "dora", "soul"].every(key => typeof rule[key] === "boolean")
            && ["allow", "exclude", "only"].includes(rule.joker);
    });
}
