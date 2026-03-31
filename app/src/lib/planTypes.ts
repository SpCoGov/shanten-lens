export type TileId = number;

export type QuadCatalogItem = {
    face?: string;
    label?: string;
    score?: string;
    switch_batch_sizes?: number[];
    switch_discards?: TileId[][];
    switch_in?: TileId[][];
    reachable?: boolean;
    reason?: string;
};

export type PlanData = {
    status?: "win_now" | "plan" | "impossible" | "searching" | "catalog";
    draws_needed?: number | null;
    target14?: string[];
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
};
