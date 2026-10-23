import {newTargetGroup, restoreTargetGroups, type TargetGroup} from "./customTarget";

export const CUSTOM_SWITCH_TARGET_KEY = "sl-custom-switch:target-v1";
export const CUSTOM_SWITCH_WALL_LIMIT_KEY = "sl-custom-switch:wall-limit";
export const CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY = "sl-custom-switch:exclude-invalid-shapes";

export function readCustomSwitchOptions() {
    let groups: TargetGroup[] = Array.from({length: 14}, newTargetGroup);
    let wallLimit = 36;
    let excludeInvalidShapes = false;
    try {
        const limit = Number(localStorage.getItem(CUSTOM_SWITCH_WALL_LIMIT_KEY) ?? "36");
        if (Number.isInteger(limit) && limit >= 0 && limit <= 36) wallLimit = limit;
        excludeInvalidShapes = localStorage.getItem(CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY) === "1";
        groups = restoreTargetGroups(JSON.parse(localStorage.getItem(CUSTOM_SWITCH_TARGET_KEY) ?? "null"));
    } catch {}
    return {search_algorithm: "custom_target" as const, target_groups: groups,
        wall_limit: wallLimit, exclude_invalid_shapes: excludeInvalidShapes};
}
