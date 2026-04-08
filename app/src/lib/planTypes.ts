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

export type WorkerState = {
    worker_id?: number;
    kind?: string;
    status?: string;
    current_quad_index?: number | null;
    current_quad_label?: string;
    current_wait_face?: string | null;
    completed_jobs?: number;
    last_result?: string;
    last_draws_needed?: number | null;
    elapsed_sec?: number;
};

export type ParallelInfo = {
    mode?: string;
    enabled?: boolean;
    max_workers?: number;
    configured_max_workers?: number;
    total_jobs?: number;
    completed_jobs?: number;
    fallback_reason?: string;
    cpu_count?: number;
    quad_pair_count?: number;
    parallel_ok?: boolean;
    attempted?: boolean;
    disabled_reason?: string;
    start_error?: string;
};

export type SearchRuntimeProcess = {
    pid?: number | null;
    alive?: boolean;
    exitcode?: number | null;
    status?: string;
};

export type SearchRuntimeData = {
    searching?: boolean;
    process_count?: number;
    updated_at?: number;
    processes?: SearchRuntimeProcess[];
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
    worker_states?: WorkerState[];
    parallel_info?: ParallelInfo;
    runtime?: SearchRuntimeData;
};
