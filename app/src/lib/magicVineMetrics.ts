import {parseFixed2} from "./scoreEngine";
import type {MagicVineGame} from "../bindings";

type VineComparisonGame = Pick<MagicVineGame,"started_at"|"pack_obtained"|"shop_obtained"|"regeneration_obtained"|"other_obtained"|"farming_obtained"|"farming_rounds"|"shops">;
export type VineSummaryMetric = "obtained" | "average" | "shops" | "purchases";

export function vineMetricTrend(current: VineComparisonGame | null, history: readonly VineComparisonGame[], metric: VineSummaryMetric): "up" | "down" | "unknown" {
    const ratio = (game: VineComparisonGame): [bigint,bigint] | null => {
        const numerator = metric === "average" ? game.farming_obtained : metric === "shops" ? game.shops : metric === "purchases" ? game.shop_obtained
            : game.pack_obtained + game.shop_obtained + game.regeneration_obtained + game.other_obtained;
        const denominator = metric === "average" ? game.farming_rounds : 1;
        return Number.isSafeInteger(numerator) && numerator >= 0 && Number.isSafeInteger(denominator) && denominator > 0
            ? [BigInt(numerator),BigInt(denominator)] : null;
    };
    const value = current && ratio(current);
    if (!value) return "unknown";
    return history.some(game => {
        if (game.started_at === current.started_at) return false;
        const previous = ratio(game);
        return previous && value[0] * previous[1] < previous[0] * value[1];
    }) ? "down" : "up";
}

export function hasObtainedMagicVine(
    game: Pick<MagicVineGame, "pack_obtained" | "shop_obtained" | "regeneration_obtained" | "other_obtained"> | null,
    amulets: readonly {id: number}[] = [],
): boolean {
    return !!game && game.pack_obtained + game.shop_obtained + game.regeneration_obtained + game.other_obtained > 0
        || amulets.some(amulet => Math.floor(amulet.id / 10) === 376);
}

export function highestTileScore(scores: Record<string, string> = {}): bigint | null {
    let highest: bigint | null = null;
    for (const raw of Object.values(scores)) {
        try {
            const score = parseFixed2(raw);
            if (highest == null || score > highest) highest = score;
        } catch {
            // A missing or malformed tile score must not hide the valid scores.
        }
    }
    return highest;
}
