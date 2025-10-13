export type TileCode = string;

export const ID_TO_TILE = new Map<number, TileCode>([
    [5, "1s"],
    [85, "6z"],
    [62, "0p"],
    [102, "bd"],
    [98, "9m"],
]);

/** 将 id 数组映射为 tile 字符串数组（未知 id 会被丢弃/或返回占位 "bd"） */
export function mapIdsToTiles(ids: number[], fallback: TileCode = "bd"): TileCode[] {
    return ids.map((id) => ID_TO_TILE.get(id) ?? fallback);
}

/** 运行时更新/注入整张映射表（例如从后端拉 JSON） */
export function setIdTileMap(entries: [number, TileCode][]) {
    ID_TO_TILE.clear();
    for (const [k, v] of entries) ID_TO_TILE.set(k, v);
}