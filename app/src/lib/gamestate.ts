export interface GameStateData {
    source?: "qyzz" | "packet" | null;
    qyzz_connected?: boolean | null;
    session_id?: number;
    revision?: number;
    flow_id?: number;
    stage: number;
    coin: string;
    point?: string;
    target_point?: string;
    level?: number;
    node?: number;
    map_nodes?: MapNodeItem[];
    deck_map: Record<string, string>;
    hand_tiles: number[];
    dora_tiles: number[];
    tian_dora_tiles?: string[];
    ming?: MingItem[];
    replacement_tiles: number[];
    wall_tiles: number[];
    ended?: boolean;
    desktop_remain: number;
    locked_tiles: number[];
    switch_used_tiles: number[];
    effect_list?: EffectItem[];
    goods?: GoodsItem[];
    refresh_price?: number;
    candidate_effect_list?: CandidateEffectRef[];
    boss_buff?: number[];
    shop_buff_list?: Record<number, number>;
    change_tile_count?: number;
    total_change_tile_count?: number;
    max_effect_volume?: number;
    tile_score_map?: Record<string, string>;
    fan_value_map?: Record<string, string>;
    character_id?: number | string;
    hp?: number | string;
    max_hp?: number | string;
    update_reason?: string[];
}

export function buildDebugSnapshotFromState(state: GameStateData) {
    return {
        stage: state.stage,
        deck_map: state.deck_map || {},
        dora_tiles: state.dora_tiles ?? [],
        tian_dora_tiles: state.tian_dora_tiles ?? [],
        hand_tiles: Array.isArray(state.hand_tiles) ? state.hand_tiles : [],
        ming: Array.isArray(state.ming) ? state.ming : [],
        replacement_tiles: Array.isArray(state.replacement_tiles) ? state.replacement_tiles : [],
        wall_tiles: Array.isArray(state.wall_tiles) ? state.wall_tiles : [],
        switch_used_tiles: Array.isArray(state.switch_used_tiles) ? state.switch_used_tiles : [],
        total_change_tile_count: state.total_change_tile_count ?? 0,
        change_tile_count: state.change_tile_count ?? 0,
        boss_buff: Array.isArray(state.boss_buff) ? state.boss_buff : [],
    };
}

export interface MingItem {
    type: number;
    tileList: number[];
}

export interface MapNodeItem {
    type?: number;
    subType?: number;
    args?: unknown[];
}

export interface BadgeAffix {
    id: number;
    uid: number;
    random: number;
    store: string[];
}

export interface EffectItem {
    id: number;
    uid: number;
    volume: 1 | 2;
    store: string[];
    tags: string[];
    badge?: BadgeAffix;
}

export interface GoodsItem {
    id: number;
    goodsId: number;
    price: number | string;
    sold: boolean;
}

export interface CandidateEffectRef {
    id: number;
    badgeId: number;
}

/** dict -> Map<number, string>（按 Object.entries 的顺序） */
export function toDeckMap(dict: Record<string, string>): Map<number, string> {
    const m = new Map<number, string>();
    for (const [k, v] of Object.entries(dict)) m.set(Number(k), v);
    return m;
}

export type Cell = { tile: string; dim: boolean; id?: number };

/**
 * 生成展示列表（仅负责“先 locked 尾→头，再 wall 尾→头”的拼接）
 * 具体“右下到左上”的最终落位在 TileGrid 内做（用 gridRow/gridColumn 定位）。
 */
export function buildCells(deck: Map<number, string>, locked: number[], wall: number[], cap = 36): Cell[] {
    const out: Cell[] = [];
    if (Array.isArray(locked)) {
        for (let i = locked.length - 1; i >= 0; i--) {
            const id = locked[i];
            out.push({tile: deck.get(id) ?? "5m", dim: true, id});
        }
    }
    if (Array.isArray(wall)) {
        for (let i = wall.length - 1; i >= 0; i--) {
            const id = wall[i];
            out.push({tile: deck.get(id) ?? "5m", dim: false, id});
        }
    }
    return out.slice(0, cap);
}
