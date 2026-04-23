export function getDoraTileFromIndicator(tile?: string | null): string | null {
    const text = String(tile ?? "").trim();
    const match = text.match(/^([0-9])([mps])$/i) ?? text.match(/^([1-7])(z)$/i);
    if (!match) return null;
    let value = Number.parseInt(match[1], 10);
    const suit = match[2].toLowerCase();
    if (value === 0 && (suit === "m" || suit === "p" || suit === "s")) {
        value = 5;
    }
    if (suit === "m") {
        if (value === 1) return "9m";
        if (value === 9) return "1m";
        return `${value + 1}m`;
    }
    if (suit === "p" || suit === "s") {
        if (value === 9) return `1${suit}`;
        return `${value + 1}${suit}`;
    }
    if (suit === "z") {
        const next = value === 7 ? 1 : value + 1;
        return `${next}z`;
    }
    return null;
}

export function buildDoraCountByTile(
    deckMap: ReadonlyMap<number, string>,
    doraTileIds?: readonly number[],
): Map<string, number> {
    const out = new Map<string, number>();
    (doraTileIds ?? []).forEach((tileId) => {
        const indicatorTile = deckMap.get(tileId);
        const doraTile = getDoraTileFromIndicator(indicatorTile);
        if (!doraTile) return;
        out.set(doraTile, (out.get(doraTile) ?? 0) + 1);
    });
    return out;
}
