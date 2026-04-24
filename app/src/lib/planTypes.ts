export type TileId = number;

export type QuadCatalogItem = {
    face?: string;
    label?: string;
    tile_positions?: Array<{
        tile_id: TileId;
        source: "hand" | "replacement" | "wall" | string;
        source_index: number;
    }>;
    score?: string;
    switch_batch_sizes?: number[];
    switch_discards?: TileId[][];
    switch_in?: TileId[][];
    reachable?: boolean;
    reason?: string;
};

export type DebugPoolTilePosition = {
    tile_id: TileId;
    source: "hand" | "replacement" | "wall" | string;
    source_index: number;
};

export type DebugPoolData = {
    focus_face?: string;
    focus_count?: number;
    raw_focus_counts?: Record<string, number>;
    pool_counts?: Record<string, number>;
    replacement_window?: {
        used_count?: number;
        total_remaining?: number;
        window_count?: number;
    };
    focus_entries?: Array<DebugPoolTilePosition & {
        raw_face?: string;
        norm_face?: string;
    }>;
    norm_face_counts?: Record<string, number>;
    available_quads?: Array<{
        face?: string;
        tile_positions?: DebugPoolTilePosition[];
    }>;
};

export type PlanData = {
    status?: "win_now" | "plan" | "impossible" | "searching" | "catalog";
    draws_needed?: number | null;
    search_algorithm?: string;
    search_algorithm_label?: string;
    target14?: string[];
    target13?: string[];
    discards?: TileId[];
    mode?: string;
    reason?: string;
    progress?: string;
    remaining_changes?: number;
    plan_signature?: string;
    switch_batch_sizes?: number[];
    switch_discards?: TileId[][];
    switch_in?: TileId[][];
    wall_draws?: TileId[];
    post_draw_discards?: TileId[];
    waits?: string[];
    quad_faces?: string[];
    quad_catalog?: QuadCatalogItem[];
    max_change_count?: number;
    per_change_limit?: number;
    considered_tile_count?: number;
    request_source?: "live" | "debug" | string;
    component_descs?: string[];
    manual_searchable?: boolean;
    manual_search_reason?: string;
    debug_pool?: DebugPoolData;
};
