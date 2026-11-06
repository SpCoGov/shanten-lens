import {parseFixed2} from "./scoreEngine";
import type {MagicVineGame} from "../bindings";

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
