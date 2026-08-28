export type TodayTile =
    | "0m" | "1m" | "2m" | "3m" | "4m" | "5m" | "6m" | "7m" | "8m" | "9m"
    | "0p" | "1p" | "2p" | "3p" | "4p" | "5p" | "6p" | "7p" | "8p" | "9p"
    | "0s" | "1s" | "2s" | "3s" | "4s" | "5s" | "6s" | "7s" | "8s" | "9s"
    | "1z" | "2z" | "3z" | "4z" | "5z" | "6z" | "7z";

export type MeldKind = "sequence" | "triplet" | "pair" | "single";
export type HandShapeKind = "standard" | "chiitoi" | "kokushi";
export type YakuId =
    | "tanyao"
    | "yakuhai"
    | "pinfu"
    | "toitoi"
    | "chiitoi"
    | "honitsu"
    | "chinitsu"
    | "kokushi"
    | "iipeikou"
    | "sanshoku"
    | "ittsuu"
    | "sanankou";
export type FeedbackColor = "green" | "yellow" | "gray";

export type HandGroup = {
    kind: MeldKind;
    tiles: TodayTile[];
};

export type HandAnalysis = {
    tiles: TodayTile[];
    groups: HandGroup[];
    shape: HandShapeKind;
    yaku: YakuId[];
};

export type TodayPuzzle = {
    dateKey: string;
    concealed: TodayTile[];
    winTile: TodayTile;
    answer: TodayTile[];
    analysis: HandAnalysis;
    waits: TodayTile[];
};

export type GuessValidation =
    | { ok: true; analysis: HandAnalysis; waits: TodayTile[]; waitMatches: boolean }
    | { ok: false; errorKey: string };

export const ALL_TILES: TodayTile[] = [
    "1m", "2m", "3m", "4m", "5m", "0m", "6m", "7m", "8m", "9m",
    "1p", "2p", "3p", "4p", "5p", "0p", "6p", "7p", "8p", "9p",
    "1s", "2s", "3s", "4s", "5s", "0s", "6s", "7s", "8s", "9s",
    "1z", "2z", "3z", "4z", "5z", "6z", "7z",
];

const CONCRETE_TILES = ALL_TILES;
const SUITS = ["m", "p", "s"] as const;
const HONORS: TodayTile[] = ["1z", "2z", "3z", "4z", "5z", "6z", "7z"];
const TERMINALS: TodayTile[] = ["1m", "9m", "1p", "9p", "1s", "9s"];
const KOKUSHI_TILES: TodayTile[] = [...TERMINALS, ...HONORS];
const YAKU_POOL: YakuId[] = ["tanyao", "yakuhai", "pinfu", "toitoi", "chiitoi", "honitsu", "chinitsu", "kokushi", "iipeikou", "sanshoku", "ittsuu", "sanankou"];

function tileSuit(tile: TodayTile) {
    return tile[1] as "m" | "p" | "s" | "z";
}

function tileNumber(tile: TodayTile) {
    const n = Number(tile[0]);
    return n === 0 ? 5 : n;
}

function normalizeForHand(tile: TodayTile): TodayTile {
    if (tile === "0m") return "5m";
    if (tile === "0p") return "5p";
    if (tile === "0s") return "5s";
    return tile;
}

function tileSortValue(tile: TodayTile) {
    const suitRank = {m: 0, p: 1, s: 2, z: 3}[tileSuit(tile)];
    return suitRank * 10 + tileNumber(tile);
}

export function sortTiles<T extends TodayTile>(tiles: T[]): T[] {
    return [...tiles].sort((a, b) => tileSortValue(a) - tileSortValue(b));
}

function makeRng(seed: string) {
    let h = 2166136261;
    for (let i = 0; i < seed.length; i++) {
        h ^= seed.charCodeAt(i);
        h = Math.imul(h, 16777619);
    }
    return () => {
        h += 0x6D2B79F5;
        let t = h;
        t = Math.imul(t ^ (t >>> 15), t | 1);
        t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
        return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
}

function pick<T>(rng: () => number, values: readonly T[]): T {
    return values[Math.floor(rng() * values.length)];
}

function shuffle<T>(rng: () => number, values: T[]) {
    const next = [...values];
    for (let i = next.length - 1; i > 0; i--) {
        const j = Math.floor(rng() * (i + 1));
        [next[i], next[j]] = [next[j], next[i]];
    }
    return next;
}

function countTiles(tiles: TodayTile[]) {
    const counts = new Map<TodayTile, number>();
    tiles.forEach((tile) => counts.set(tile, (counts.get(tile) ?? 0) + 1));
    return counts;
}

export function hasTileOverflow(tiles: TodayTile[]) {
    const counts = countTiles(tiles);
    if ((counts.get("0m") ?? 0) > 1 || (counts.get("0p") ?? 0) > 1 || (counts.get("0s") ?? 0) > 1) return true;
    if ((counts.get("5m") ?? 0) > 3 || (counts.get("5p") ?? 0) > 3 || (counts.get("5s") ?? 0) > 3) return true;
    const normalized = new Map<TodayTile, number>();
    tiles.forEach((tile) => {
        const key = normalizeForHand(tile);
        normalized.set(key, (normalized.get(key) ?? 0) + 1);
    });
    return [...normalized.values()].some((count) => count > 4);
}

function tilesKey(tiles: TodayTile[]) {
    return sortTiles(tiles).join(",");
}

export function handCalcKey(tiles: TodayTile[]) {
    return tilesKey(tiles.map(normalizeForHand));
}

function groupKey(group: HandGroup) {
    return `${group.kind}:${tilesKey(group.tiles)}`;
}

function isTerminalOrHonor(tile: TodayTile) {
    return tileSuit(tile) === "z" || tileNumber(tile) === 1 || tileNumber(tile) === 9;
}

function isSimple(tile: TodayTile) {
    return tileSuit(tile) !== "z" && tileNumber(tile) >= 2 && tileNumber(tile) <= 8;
}

function isSequence(tiles: TodayTile[]) {
    if (tiles.length !== 3) return false;
    const sorted = sortTiles(tiles);
    const suit = tileSuit(sorted[0]);
    if (suit === "z" || sorted.some((tile) => tileSuit(tile) !== suit || tileNumber(tile) === 0)) return false;
    return tileNumber(sorted[1]) === tileNumber(sorted[0]) + 1 && tileNumber(sorted[2]) === tileNumber(sorted[1]) + 1;
}

function sequence(start: number, suit: "m" | "p" | "s"): TodayTile[] {
    return [`${start}${suit}`, `${start + 1}${suit}`, `${start + 2}${suit}`] as TodayTile[];
}

function triplet(tile: TodayTile): TodayTile[] {
    return [tile, tile, tile];
}

function pair(tile: TodayTile): TodayTile[] {
    return [tile, tile];
}

function decomposeStandard(tiles: TodayTile[]): HandGroup[] {
    const counts = countTiles(tiles.map(normalizeForHand));
    const out: HandGroup[][] = [];

    const searchMelds = (currentCounts: Map<TodayTile, number>, groups: HandGroup[]) => {
        const first = CONCRETE_TILES.find((tile) => (currentCounts.get(tile) ?? 0) > 0);
        if (!first) {
            if (groups.length === 5) out.push(groups);
            return;
        }
        const count = currentCounts.get(first) ?? 0;
        if (count >= 3) {
            currentCounts.set(first, count - 3);
            searchMelds(currentCounts, [...groups, {kind: "triplet", tiles: triplet(first)}]);
            currentCounts.set(first, count);
        }
        const suit = tileSuit(first);
        const n = tileNumber(first);
        if (suit !== "z" && n >= 1 && n <= 7) {
            const b = `${n + 1}${suit}` as TodayTile;
            const c = `${n + 2}${suit}` as TodayTile;
            if ((currentCounts.get(b) ?? 0) > 0 && (currentCounts.get(c) ?? 0) > 0) {
                currentCounts.set(first, count - 1);
                currentCounts.set(b, (currentCounts.get(b) ?? 0) - 1);
                currentCounts.set(c, (currentCounts.get(c) ?? 0) - 1);
                searchMelds(currentCounts, [...groups, {kind: "sequence", tiles: [first, b, c]}]);
                currentCounts.set(first, count);
                currentCounts.set(b, (currentCounts.get(b) ?? 0) + 1);
                currentCounts.set(c, (currentCounts.get(c) ?? 0) + 1);
            }
        }
    };

    CONCRETE_TILES.forEach((tile) => {
        const count = counts.get(tile) ?? 0;
        if (count < 2) return;
        const next = new Map(counts);
        next.set(tile, count - 2);
        searchMelds(next, [{kind: "pair", tiles: pair(tile)}]);
    });

    return out.sort((a, b) => scoreGroupsForFeedback(b) - scoreGroupsForFeedback(a))[0] ?? [];
}

function scoreGroupsForFeedback(groups: HandGroup[]) {
    return groups.reduce((score, group) => score + (group.kind === "sequence" ? 2 : group.kind === "triplet" ? 3 : 1), 0);
}

function decomposeChiitoi(tiles: TodayTile[]): HandGroup[] {
    const counts = countTiles(tiles.map(normalizeForHand));
    if (counts.size !== 7 || [...counts.values()].some((count) => count !== 2)) return [];
    return sortTiles([...counts.keys()]).map((tile) => ({kind: "pair" as const, tiles: pair(tile)}));
}

function decomposeKokushi(tiles: TodayTile[]): HandGroup[] {
    const counts = countTiles(tiles.map(normalizeForHand));
    const hasAll = KOKUSHI_TILES.every((tile) => (counts.get(tile) ?? 0) >= 1);
    const extra = KOKUSHI_TILES.find((tile) => (counts.get(tile) ?? 0) === 2);
    if (!hasAll || !extra || counts.size !== 13) return [];
    return KOKUSHI_TILES.map((tile) => ({kind: tile === extra ? "pair" : "single", tiles: tile === extra ? pair(tile) : [tile]}));
}

export function analyzeHand(tiles: TodayTile[]): HandAnalysis | null {
    if (tiles.length !== 14 || hasTileOverflow(tiles)) return null;
    const normalizedTiles = sortTiles(tiles.map(normalizeForHand));
    const kokushi = decomposeKokushi(tiles);
    if (kokushi.length) return withYaku({tiles: normalizedTiles, groups: kokushi, shape: "kokushi", yaku: ["kokushi"]});
    const chiitoi = decomposeChiitoi(tiles);
    if (chiitoi.length) return withYaku({tiles: normalizedTiles, groups: chiitoi, shape: "chiitoi", yaku: ["chiitoi"]});
    const standard = decomposeStandard(tiles);
    if (standard.length !== 5) return null;
    return withYaku({tiles: normalizedTiles, groups: standard, shape: "standard", yaku: []});
}

function withYaku(analysis: HandAnalysis): HandAnalysis {
    const yaku = new Set<YakuId>(analysis.yaku);
    const tiles = analysis.tiles;
    const groups = analysis.groups;
    const melds = groups.filter((group) => group.kind === "sequence" || group.kind === "triplet");
    if (tiles.every(isSimple)) yaku.add("tanyao");
    if (groups.some((group) => group.kind === "triplet" && tileSuit(group.tiles[0]) === "z")) yaku.add("yakuhai");
    if (analysis.shape === "standard" && melds.every((group) => group.kind === "triplet")) yaku.add("toitoi");
    if (analysis.shape === "standard" && melds.every((group) => group.kind === "sequence") && groups.find((group) => group.kind === "pair")?.tiles.every(isSimple)) yaku.add("pinfu");
    if (analysis.shape === "standard" && melds.filter((group) => group.kind === "triplet").length >= 3) yaku.add("sanankou");
    const suits = new Set(tiles.filter((tile) => tileSuit(tile) !== "z").map(tileSuit));
    if (suits.size === 1 && tiles.every((tile) => tileSuit(tile) !== "z")) yaku.add("chinitsu");
    if (suits.size === 1 && tiles.some((tile) => tileSuit(tile) === "z")) yaku.add("honitsu");
    const sequenceKeys = melds.filter((group) => group.kind === "sequence").map((group) => tilesKey(group.tiles));
    if (new Set(sequenceKeys).size < sequenceKeys.length) yaku.add("iipeikou");
    for (let start = 1; start <= 7; start++) {
        if (SUITS.every((suit) => sequenceKeys.includes(tilesKey(sequence(start, suit))))) yaku.add("sanshoku");
    }
    for (const suit of SUITS) {
        if ([1, 4, 7].every((start) => sequenceKeys.includes(tilesKey(sequence(start, suit))))) yaku.add("ittsuu");
    }
    return {...analysis, yaku: [...yaku]};
}

export function calculateWaits(concealed: TodayTile[]): TodayTile[] {
    if (concealed.length !== 13) return [];
    const counts = countTiles(concealed);
    const waits = CONCRETE_TILES.filter((tile) => (counts.get(tile) ?? 0) < 4)
        .filter((tile) => !hasTileOverflow([...concealed, tile]))
        .filter((tile) => {
            const analysis = analyzeHand([...concealed, tile]);
            return Boolean(analysis && analysis.yaku.length > 0);
        });
    return sortTiles([...new Map(waits.map((tile) => [normalizeForHand(tile), normalizeForHand(tile)])).values()]);
}

function sameSet(a: TodayTile[], b: TodayTile[]) {
    return handCalcKey(a) === handCalcKey(b);
}

export function validateGuess(concealed: TodayTile[], winTile: TodayTile | null, answerWaits: TodayTile[]): GuessValidation {
    if (concealed.length < 13) return {ok: false, errorKey: "today_win.errors.short_hand"};
    if (concealed.length > 13) return {ok: false, errorKey: "today_win.errors.too_many_hand"};
    if (!winTile) return {ok: false, errorKey: "today_win.errors.missing_win"};
    const tiles = [...concealed, winTile];
    if (hasTileOverflow(tiles)) return {ok: false, errorKey: "today_win.errors.tile_overflow"};
    const analysis = analyzeHand(tiles);
    if (!analysis) return {ok: false, errorKey: "today_win.errors.not_win"};
    if (analysis.yaku.length === 0) return {ok: false, errorKey: "today_win.errors.no_yaku"};
    const waits = calculateWaits(concealed);
    if (!waits.includes(normalizeForHand(winTile))) return {ok: false, errorKey: "today_win.errors.bad_win_tile"};
    return {ok: true, analysis, waits, waitMatches: sameSet(waits, answerWaits)};
}

export function buildFeedback(guessTiles: TodayTile[], answerTiles: TodayTile[], guess: HandAnalysis, answer: HandAnalysis, guessWinTile: TodayTile, answerWinTile: TodayTile): FeedbackColor[] {
    const colors: FeedbackColor[] = Array(14).fill("gray");
    const remaining = countTiles(answerTiles);
    const answerGroups = new Set(answer.groups.map(groupKey));

    guessTiles.forEach((tile, index) => {
        if (colors[index] !== "gray") return;
        if (index === 13 && tile !== answerWinTile) return;
        const belongsGreenGroup = guess.groups.some((group) => group.tiles.includes(normalizeForHand(tile)) && answerGroups.has(groupKey(group)));
        if (belongsGreenGroup && (remaining.get(tile) ?? 0) > 0) {
            colors[index] = "green";
            remaining.set(tile, (remaining.get(tile) ?? 0) - 1);
        }
    });

    if (guessWinTile === answerWinTile) {
        const index = guessTiles.length - 1;
        if (index >= 0 && colors[index] !== "green" && (remaining.get(guessWinTile) ?? 0) > 0) {
            colors[index] = "green";
            remaining.set(guessWinTile, (remaining.get(guessWinTile) ?? 0) - 1);
        }
    }

    guessTiles.forEach((tile, index) => {
        if (colors[index] !== "gray") return;
        if ((remaining.get(tile) ?? 0) > 0) {
            colors[index] = "yellow";
            remaining.set(tile, (remaining.get(tile) ?? 0) - 1);
        }
    });
    return colors;
}

function candidateFromGroups(groups: TodayTile[][], pairTiles: TodayTile[]): TodayTile[] {
    return sortTiles([...groups.flat(), ...pairTiles]);
}

function randomStandardForYaku(rng: () => number, yaku: YakuId): TodayTile[] {
    const groups: TodayTile[][] = [];
    let pairTiles: TodayTile[] = pair(pick(rng, CONCRETE_TILES.filter((tile) => !isTerminalOrHonor(tile))));
    if (yaku === "pinfu") {
        const suit = pick(rng, SUITS);
        groups.push(sequence(2, suit), sequence(3, pick(rng, SUITS)), sequence(4, pick(rng, SUITS)), sequence(6, pick(rng, SUITS)));
        pairTiles = pair(pick(rng, ["2m", "3p", "4s", "6m", "7p", "8s"] as TodayTile[]));
    } else if (yaku === "toitoi") {
        groups.push(...shuffle(rng, CONCRETE_TILES).slice(0, 4).map(triplet));
    } else if (yaku === "yakuhai") {
        groups.push(triplet(pick(rng, HONORS)));
    } else if (yaku === "sanankou") {
        groups.push(...shuffle(rng, CONCRETE_TILES).slice(0, 3).map(triplet));
    } else if (yaku === "honitsu" || yaku === "chinitsu") {
        const suit = pick(rng, SUITS);
        const suited = [1, 2, 3, 4, 5, 6, 7, 8, 9].map((n) => `${n}${suit}` as TodayTile);
        groups.push(sequence(1, suit), sequence(4, suit), triplet(pick(rng, suited)));
        if (yaku === "honitsu") groups.push(triplet(pick(rng, HONORS)));
        pairTiles = pair(pick(rng, yaku === "honitsu" ? [...suited, ...HONORS] : suited));
    } else if (yaku === "iipeikou") {
        const suit = pick(rng, SUITS);
        const seq = sequence(pick(rng, [1, 2, 3, 4, 5, 6, 7]), suit);
        groups.push(seq, seq);
    } else if (yaku === "sanshoku") {
        const start = pick(rng, [1, 2, 3, 4, 5, 6, 7]);
        groups.push(sequence(start, "m"), sequence(start, "p"), sequence(start, "s"));
    } else if (yaku === "ittsuu") {
        const suit = pick(rng, SUITS);
        groups.push(sequence(1, suit), sequence(4, suit), sequence(7, suit));
    } else {
        groups.push(sequence(2, pick(rng, SUITS)));
    }
    while (groups.length < 4) {
        const suit = pick(rng, SUITS);
        groups.push(rng() < 0.7 ? sequence(pick(rng, [1, 2, 3, 4, 5, 6, 7]), suit) : triplet(pick(rng, CONCRETE_TILES)));
    }
    return candidateFromGroups(groups.slice(0, 4), pairTiles);
}

function randomChiitoi(rng: () => number) {
    return sortTiles(shuffle(rng, CONCRETE_TILES).slice(0, 7).flatMap(pair));
}

function randomKokushi(rng: () => number) {
    const pairTile = pick(rng, KOKUSHI_TILES);
    return sortTiles([...KOKUSHI_TILES, pairTile]);
}

function buildCandidate(rng: () => number, yaku: YakuId) {
    if (yaku === "chiitoi") return randomChiitoi(rng);
    if (yaku === "kokushi") return randomKokushi(rng);
    return randomStandardForYaku(rng, yaku);
}

export function getTodayKey(date = new Date()) {
    const yyyy = date.getFullYear();
    const mm = String(date.getMonth() + 1).padStart(2, "0");
    const dd = String(date.getDate()).padStart(2, "0");
    return `${yyyy}-${mm}-${dd}`;
}

export function generateTodayPuzzle(dateKey = getTodayKey()): TodayPuzzle {
    const rng = makeRng(`today-win:${dateKey}`);
    const yakuOrder = shuffle(rng, YAKU_POOL);
    for (let round = 0; round < 80; round++) {
        const yaku = yakuOrder[round % yakuOrder.length];
        const answer = buildCandidate(rng, yaku);
        const analysis = analyzeHand(answer);
        if (!analysis || analysis.yaku.length === 0 || !analysis.yaku.includes(yaku) || hasTileOverflow(answer)) continue;
        for (const winTile of shuffle(rng, [...new Set(answer)])) {
            const concealed = [...answer];
            concealed.splice(concealed.indexOf(winTile), 1);
            const waits = calculateWaits(concealed);
            if (waits.includes(winTile)) {
                return {dateKey, concealed: sortTiles(concealed), winTile, answer: sortTiles(answer), analysis, waits};
            }
        }
    }
    const fallback: TodayTile[] = ["2m", "3m", "4m", "2p", "3p", "4p", "2s", "3s", "4s", "6m", "7m", "8m", "5s", "5s"];
    const analysis = analyzeHand(fallback)!;
    const concealed = fallback.slice(0, 13);
    return {dateKey, concealed, winTile: "5s", answer: fallback, analysis, waits: calculateWaits(concealed)};
}
