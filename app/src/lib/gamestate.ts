export interface GameStateData {
    stage: number;
    deck_map: Record<string, string>;   // id(string) -> tile("1m"/"0p"/"6z"/"bd"...)
    hand_tiles: number[];
    dora_tiles: number[];
    replacement_tiles: number[];
    wall_tiles: number[];
    ended: boolean;
    desktop_remain: number;
    locked_tiles: number[];
}

export interface WsEnvelope<T = any> {
    type: string;
    data: T;
}

/** dict -> Map<number, string>（按 Object.entries 的顺序） */
export function toDeckMap(dict: Record<string, string>): Map<number, string> {
    const m = new Map<number, string>();
    for (const [k, v] of Object.entries(dict)) m.set(Number(k), v);
    return m;
}

export type Cell = { tile: string; dim: boolean };

/**
 * 生成展示列表（仅负责“先 locked 尾→头，再 wall 尾→头”的拼接）
 * 具体“右下到左上”的最终落位在 TileGrid 内做（用 gridRow/gridColumn 定位）。
 */
export function buildCells(deck: Map<number, string>, locked: number[], wall: number[], cap = 36): Cell[] {
    const out: Cell[] = [];
    if (Array.isArray(locked)) {
        for (let i = locked.length - 1; i >= 0; i--) {
            const id = locked[i];
            out.push({ tile: deck.get(id) ?? "5m", dim: true });
        }
    }
    if (Array.isArray(wall)) {
        for (let i = wall.length - 1; i >= 0; i--) {
            const id = wall[i];
            out.push({ tile: deck.get(id) ?? "5m", dim: false });
        }
    }
    return out.slice(0, cap);
}
