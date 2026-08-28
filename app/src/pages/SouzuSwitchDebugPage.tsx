import React from "react";
import Tile from "../components/Tile";
import {useTranslation} from "react-i18next";
import {pushToast} from "../lib/toast";
import type {GameStateData} from "../lib/gamestate";
import type {PlanData} from "../lib/planTypes";
import * as backendIpc from "../lib/ipc";
import {BlackHoleStrategyCard} from "./BlackHolePage";

export type DebugSnapshot = {
    stage?: number;
    deck_map: Record<string, string>;
    hand_tiles: number[];
    ming?: {type: number; tileList: number[]}[];
    replacement_tiles: number[];
    wall_tiles: number[];
    switch_used_tiles: number[];
    total_change_tile_count?: number;
    change_tile_count?: number;
    boss_buff?: number[];
};

type PoolSource = "hand" | "replacement" | "wall";
type PoolEntry = {
    tileId: number;
    face: string;
    source: PoolSource;
    sourceIndex: number;
    order: number;
};

type QuadOption = {
    key: string;
    face: string;
    ids: number[];
    label: string;
};

type ManualStructureKey = "meld1" | "meld2" | "pair";
type ManualStructureState = Record<ManualStructureKey, number[]>;

const DEFAULT_WALL_LIMIT = 36;
const WALL_LIMIT_MIN = 2;
const WALL_LIMIT_MAX = 36;
const DEFAULT_SEARCH_ALGORITHM = "target_enumeration_search";
const RED_MAP: Record<string, string> = {"0m": "5m", "0p": "5p", "0s": "5s"};
const STRUCTURE_LIMITS: Record<ManualStructureKey, number> = {
    meld1: 3,
    meld2: 3,
    pair: 2,
};

function normFace(face: string | null | undefined) {
    if (!face) return "?";
    return RED_MAP[face] || face;
}

function parseWallLimitInput(raw: string, t: (key: string) => string) {
    const value = Number(raw);
    if (!Number.isFinite(value) || !Number.isInteger(value)) {
        return {ok: false as const, message: t("blackhole.wall_limit_error_integer")};
    }
    if (value < WALL_LIMIT_MIN || value > WALL_LIMIT_MAX) {
        return {ok: false as const, message: t("blackhole.wall_limit_error_range")};
    }
    return {ok: true as const, value};
}


export function buildDebugSnapshotFromState(state: GameStateData): DebugSnapshot {
    return {
        stage: state.stage,
        deck_map: state.deck_map || {},
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

function safeParseSnapshot(text: string, t: (key: string) => string): DebugSnapshot {
    const parsed = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error(t("souzu_debug.json_object_required"));
    }
    return parsed as DebugSnapshot;
}

function resolveFaceFactory(snapshot: DebugSnapshot | null) {
    const deckMap = snapshot?.deck_map || {};
    return (id: number) => {
        const direct = deckMap[String(id)];
        if (typeof direct === "string") return direct;
        const numeric = (deckMap as Record<number, string>)[id];
        return typeof numeric === "string" ? numeric : null;
    };
}

function combinations<T>(items: T[], size: number): T[][] {
    if (size <= 0) return [[]];
    if (items.length < size) return [];
    if (size === 1) return items.map((item) => [item]);
    const out: T[][] = [];
    items.forEach((item, index) => {
        combinations(items.slice(index + 1), size - 1).forEach((rest) => {
            out.push([item, ...rest]);
        });
    });
    return out;
}

function sourceShort(source: PoolSource) {
    if (source === "hand") return "H";
    if (source === "replacement") return "R";
    return "W";
}

function sourceTitle(source: PoolSource, t: (key: string) => string) {
    return t(`souzu_debug.source_${source}`);
}

function bucketTitle(key: ManualStructureKey, t: (key: string) => string) {
    return t(`souzu_debug.bucket_${key}`);
}

function emptyStructureState(): ManualStructureState {
    return {meld1: [], meld2: [], pair: []};
}

function allManualIds(structure: ManualStructureState) {
    return new Set([...structure.meld1, ...structure.meld2, ...structure.pair]);
}

function buildPoolEntries(snapshot: DebugSnapshot, wallLimit: number): PoolEntry[] {
    const deckMap = snapshot.deck_map || {};
    const usedCount = Array.isArray(snapshot.switch_used_tiles) ? snapshot.switch_used_tiles.length : 0;
    const remainingChanges = Math.max(0, Number(snapshot.total_change_tile_count || 0) - Number(snapshot.change_tile_count || 0));
    const perChangeLimit = (snapshot.boss_buff || []).includes(901) ? 3 : 13;
    const replacementReadLimit = Math.min(
        Math.max(0, (snapshot.replacement_tiles || []).length - usedCount),
        remainingChanges * perChangeLimit,
    );
    const replacementTiles = (snapshot.replacement_tiles || []).slice(usedCount, usedCount + replacementReadLimit);
    const wallTiles = (snapshot.wall_tiles || []).slice(0, wallLimit);
    const entries: PoolEntry[] = [];

    const pushGroup = (source: PoolSource, ids: number[]) => {
        ids.forEach((tileId, sourceIndex) => {
            const rawFace = deckMap[String(tileId)] ?? (deckMap as Record<number, string>)[tileId] ?? "?";
            const face = normFace(rawFace);
            entries.push({
                tileId,
                face,
                source,
                sourceIndex,
                order: entries.length,
            });
        });
    };

    pushGroup("hand", snapshot.hand_tiles || []);
    pushGroup("replacement", replacementTiles);
    pushGroup("wall", wallTiles);
    return entries;
}

function buildQuadOptions(entries: PoolEntry[]): QuadOption[] {
    const byFace = new Map<string, PoolEntry[]>();
    entries.forEach((entry) => {
        const bucket = byFace.get(entry.face) || [];
        bucket.push(entry);
        byFace.set(entry.face, bucket);
    });

    const out: QuadOption[] = [];
    Array.from(byFace.entries())
        .sort((a, b) => a[0].localeCompare(b[0]))
        .forEach(([face, faceEntries]) => {
            if (faceEntries.length < 4) return;
            combinations(faceEntries, 4).forEach((combo, index) => {
                const ids = combo.map((item) => item.tileId).sort((a, b) => a - b);
                out.push({
                    key: `${face}:${ids.join(",")}`,
                    face,
                    ids,
                    label: `${face} #${index + 1} (${combo.map((item) => `${sourceShort(item.source)}${item.sourceIndex + 1}`).join(" / ")})`,
                });
            });
        });
    return out;
}

export default function SouzuSwitchDebugPage({
    currentState,
    data,
    onClear,
}: {
    currentState: GameStateData | null;
    data: PlanData | null;
    onClear?: () => void;
}) {
    const {t} = useTranslation();
    const [snapshotText, setSnapshotText] = React.useState("");
    const [activeSnapshot, setActiveSnapshot] = React.useState<DebugSnapshot | null>(null);
    const [wallLimit, setWallLimit] = React.useState(DEFAULT_WALL_LIMIT);
    const [wallLimitInput, setWallLimitInput] = React.useState(String(DEFAULT_WALL_LIMIT));
    const searchAlgorithm = DEFAULT_SEARCH_ALGORITHM;
    const [selectedQuadKeys, setSelectedQuadKeys] = React.useState<string[]>(["", ""]);
    const [activeBucket, setActiveBucket] = React.useState<ManualStructureKey>("meld1");
    const [manualStructure, setManualStructure] = React.useState<ManualStructureState>(emptyStructureState);

    const resolveFace = React.useMemo(() => resolveFaceFactory(activeSnapshot), [activeSnapshot]);
    const poolEntries = React.useMemo(
        () => (activeSnapshot ? buildPoolEntries(activeSnapshot, wallLimit) : []),
        [activeSnapshot, wallLimit],
    );
    const quadOptions = React.useMemo(() => buildQuadOptions(poolEntries), [poolEntries]);
    const quadMap = React.useMemo(() => new Map(quadOptions.map((item) => [item.key, item])), [quadOptions]);
    const selectedQuads = React.useMemo(
        () => selectedQuadKeys.map((key) => quadMap.get(key) || null),
        [quadMap, selectedQuadKeys],
    );
    const selectedQuadIds = React.useMemo(() => {
        const ids = new Set<number>();
        selectedQuads.forEach((quad) => quad?.ids.forEach((id) => ids.add(id)));
        return ids;
    }, [selectedQuads]);
    const manualIds = React.useMemo(() => allManualIds(manualStructure), [manualStructure]);
    const groupedPoolEntries = React.useMemo(
        () => ({
            hand: poolEntries.filter((entry) => entry.source === "hand"),
            replacement: poolEntries.filter((entry) => entry.source === "replacement"),
            wall: poolEntries.filter((entry) => entry.source === "wall"),
        }),
        [poolEntries],
    );


    const resetManualBuilder = React.useCallback(() => {
        setSelectedQuadKeys(["", ""]);
        setManualStructure(emptyStructureState());
        setActiveBucket("meld1");
    }, []);

    const importSnapshot = React.useCallback(() => {
        try {
            const parsed = safeParseSnapshot(snapshotText, t);
            setActiveSnapshot(parsed);
            resetManualBuilder();
            pushToast(t("blackhole.debug_import_success"), "success", 1400);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : t("blackhole.debug_import_failed"), "error", 2200);
        }
    }, [resetManualBuilder, snapshotText, t]);

    const exportCurrent = React.useCallback(async () => {
        if (!currentState) {
            pushToast(t("blackhole.export_empty"), "error", 1800);
            return;
        }
        const snapshot = buildDebugSnapshotFromState(currentState);
        const nextText = JSON.stringify(snapshot);
        setSnapshotText(nextText);
        setActiveSnapshot(snapshot);
        resetManualBuilder();
        try {
            await navigator.clipboard.writeText(nextText);
            pushToast(t("blackhole.export_success_clipboard"), "success", 1600);
        } catch {
            pushToast(t("blackhole.export_success"), "success", 1400);
        }
    }, [currentState, resetManualBuilder, t]);

    const resolveWallLimit = React.useCallback(() => {
        const parsed = parseWallLimitInput(wallLimitInput.trim(), t);
        if (!parsed.ok) {
            pushToast(parsed.message, "error", 2200);
            return null;
        }
        setWallLimit(parsed.value);
        setWallLimitInput(String(parsed.value));
        return parsed.value;
    }, [t, wallLimitInput]);

    const runImportedSnapshot = React.useCallback(() => {
        let snapshot: DebugSnapshot;
        try {
            snapshot = safeParseSnapshot(snapshotText, t);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : t("souzu_debug.import_invalid"), "error", 2200);
            return;
        }
        const nextWallLimit = resolveWallLimit();
        if (nextWallLimit == null) return;
        setActiveSnapshot(snapshot);
        void backendIpc.runSwitch({
            action: "start_debug",
            snapshot,
            options: {
                skip_signatures: [],
                wall_limit: nextWallLimit,
                search_algorithm: searchAlgorithm,
            },
        });
    }, [resolveWallLimit, searchAlgorithm, snapshotText, t]);

    const validateManualPlan = React.useCallback(() => {
        let snapshot: DebugSnapshot;
        try {
            snapshot = activeSnapshot ?? safeParseSnapshot(snapshotText, t);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : t("souzu_debug.import_first"), "error", 2200);
            return;
        }
        if (!selectedQuads[0] || !selectedQuads[1]) {
            pushToast(t("souzu_debug.choose_two_quads"), "error", 1800);
            return;
        }
        if (selectedQuads[0].key === selectedQuads[1].key) {
            pushToast(t("souzu_debug.quads_must_differ"), "error", 1800);
            return;
        }
        const totalTiles = manualStructure.meld1.length + manualStructure.meld2.length + manualStructure.pair.length;
        if (totalTiles !== 7) {
            pushToast(t("souzu_debug.target_seven_tiles"), "error", 1800);
            return;
        }
        const nextWallLimit = resolveWallLimit();
        if (nextWallLimit == null) return;
        void backendIpc.runSwitch({
            action: "validate_manual_debug",
            snapshot,
            quadGroups: [selectedQuads[0].ids, selectedQuads[1].ids],
            structureGroups: manualStructure,
            options: {wall_limit: nextWallLimit},
        });
    }, [activeSnapshot, manualStructure, resolveWallLimit, selectedQuads, snapshotText, t]);

    const clearDebugState = React.useCallback(() => {
        setSnapshotText("");
        setActiveSnapshot(null);
        resetManualBuilder();
        onClear?.();
    }, [onClear, resetManualBuilder]);
    const stopSearch = React.useCallback(() => {
        void backendIpc.runSwitch({action: "stop"});
    }, []);

    React.useEffect(() => {
        setManualStructure((prev) => ({
            meld1: prev.meld1.filter((id) => !selectedQuadIds.has(id)),
            meld2: prev.meld2.filter((id) => !selectedQuadIds.has(id)),
            pair: prev.pair.filter((id) => !selectedQuadIds.has(id)),
        }));
    }, [selectedQuadIds]);

    const assignTile = React.useCallback((tileId: number) => {
        if (selectedQuadIds.has(tileId)) return;
        setManualStructure((prev) => {
            const next: ManualStructureState = {
                meld1: [...prev.meld1],
                meld2: [...prev.meld2],
                pair: [...prev.pair],
            };
            (Object.keys(next) as ManualStructureKey[]).forEach((key) => {
                next[key] = next[key].filter((id) => id !== tileId);
            });
            if (next[activeBucket].length >= STRUCTURE_LIMITS[activeBucket]) {
                pushToast(t("souzu_debug.group_full"), "error", 1400);
                return prev;
            }
            next[activeBucket].push(tileId);
            return next;
        });
    }, [activeBucket, selectedQuadIds, t]);

    const removeTile = React.useCallback((bucket: ManualStructureKey, tileId: number) => {
        setManualStructure((prev) => ({
            ...prev,
            [bucket]: prev[bucket].filter((id) => id !== tileId),
        }));
    }, []);

    const totalTiles = manualStructure.meld1.length + manualStructure.meld2.length + manualStructure.pair.length;

    return (
        <div className="settings-wrap wide-page" style={{paddingBlock: 16}}>
            <div className="panel">
                <div className="panel-title">{t("blackhole.debug_title")}</div>
                <div style={{display: "flex", flexWrap: "wrap", gap: 12, alignItems: "center"}}>
                    <button className="nav-btn" onClick={exportCurrent}>{t("blackhole.export_current")}</button>
                    <button className="nav-btn" onClick={importSnapshot}>{t("blackhole.debug_import_snapshot")}</button>
                    <button className="nav-btn" onClick={runImportedSnapshot}>{t("blackhole.debug_run_snapshot")}</button>
                    <button className="nav-btn" onClick={validateManualPlan}>{t("blackhole.debug_validate_manual")}</button>
                    <button className="nav-btn" onClick={clearDebugState}>{t("blackhole.debug_clear")}</button>
                    <button className="nav-btn" onClick={stopSearch}>{t("blackhole.stop")}</button>
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <span>{t("blackhole.wall_limit")}</span>
                        <input
                            className="form-input"
                            style={{width: 88}}
                            type="number"
                            value={wallLimitInput}
                            onChange={(e) => setWallLimitInput(e.target.value)}
                        />
                    </label>
                </div>
                <div style={{marginTop: 10, color: "var(--muted)", fontSize: 13}}>
                    {t("blackhole.debug_hint")}
                </div>
            </div>

            <div className="responsive-two-col">
                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">{t("blackhole.debug_snapshot_data")}</div>
                    <textarea
                        className="form-input"
                        style={{minHeight: 300, maxWidth: "none", fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"}}
                        value={snapshotText}
                        onChange={(e) => setSnapshotText(e.target.value)}
                        placeholder='{"stage":2,"deck_map":{},"hand_tiles":[]}'
                    />
                </section>

                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">{t("blackhole.debug_snapshot_preview")}</div>
                    {activeSnapshot ? (
                        <div style={{display: "grid", gap: 12}}>
                            <SnapshotTileRow title={t("souzu_debug.source_hand")} ids={activeSnapshot.hand_tiles ?? []} resolveFace={resolveFace}/>
                            <SnapshotTileRow title={t("souzu_debug.source_replacement")} ids={activeSnapshot.replacement_tiles ?? []} resolveFace={resolveFace}/>
                            <SnapshotTileRow title={t("souzu_debug.source_wall")} ids={(activeSnapshot.wall_tiles ?? []).slice(0, wallLimit)} resolveFace={resolveFace}/>
                            <div style={{display: "flex", flexWrap: "wrap", gap: 10}}>
                                <span className="badge">{t("souzu_debug.stage", {value: String(activeSnapshot.stage ?? "-")})}</span>
                                <span className="badge">{t("souzu_debug.changed", {value: String(activeSnapshot.change_tile_count ?? 0)})}</span>
                                <span className="badge">{t("souzu_debug.total_changes", {value: String(activeSnapshot.total_change_tile_count ?? 0)})}</span>
                                <span className="badge">Boss Buff: {(activeSnapshot.boss_buff ?? []).join(", ") || "-"}</span>
                            </div>
                        </div>
                    ) : (
                        <div className="hint">{t("blackhole.debug_snapshot_empty")}</div>
                    )}
                </section>
            </div>

            {activeSnapshot && (
                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">{t("blackhole.debug_manual_title")}</div>
                    <div style={{display: "grid", gap: 16}}>
                        <div style={{display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12}}>
                            <ManualQuadSelect
                                title={t("souzu_debug.quad_a")}
                                value={selectedQuadKeys[0]}
                                options={quadOptions}
                                onChange={(value) => setSelectedQuadKeys(([_, second]) => [value, second])}
                            />
                            <ManualQuadSelect
                                title={t("souzu_debug.quad_b")}
                                value={selectedQuadKeys[1]}
                                options={quadOptions}
                                onChange={(value) => setSelectedQuadKeys(([first]) => [first, value])}
                            />
                        </div>

                        <div style={{display: "grid", gap: 10}}>
                            <div className="hint">{t("souzu_debug.assign_to")}</div>
                            <div style={{display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center"}}>
                                {(["meld1", "meld2", "pair"] as ManualStructureKey[]).map((key) => (
                                    <button
                                        key={key}
                                        className={`nav-btn ${activeBucket === key ? "active" : ""}`}
                                        onClick={() => setActiveBucket(key)}
                                    >
                                        {bucketTitle(key, t)} {manualStructure[key].length}/{STRUCTURE_LIMITS[key]}
                                    </button>
                                ))}
                                <button className="nav-btn" onClick={resetManualBuilder}>{t("souzu_debug.reset_groups")}</button>
                                <span className="badge">{t("souzu_debug.current_total", {count: totalTiles})}</span>
                            </div>
                        </div>

                        <div style={{display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12}}>
                            {(["meld1", "meld2", "pair"] as ManualStructureKey[]).map((key) => (
                                <ManualBucket
                                    key={key}
                                    title={bucketTitle(key, t)}
                                    ids={manualStructure[key]}
                                    resolveFace={resolveFace}
                                    onRemove={(tileId) => removeTile(key, tileId)}
                                />
                            ))}
                        </div>

                        <div style={{display: "grid", gap: 12}}>
                            <div className="hint">{t("souzu_debug.allocatable_tiles")}</div>
                            {(["hand", "replacement", "wall"] as PoolSource[]).map((source) => (
                                <PoolSection
                                    key={source}
                                    title={sourceTitle(source, t)}
                                    entries={groupedPoolEntries[source]}
                                    usedQuadIds={selectedQuadIds}
                                    manualIds={manualIds}
                                    onAssign={assignTile}
                                />
                            ))}
                        </div>
                    </div>
                </section>
            )}

            <div className="blackhole-layout">
                <div className="blackhole-main">
                    <ManualSearchabilityCard data={data}/>
                    <DebugPoolCard data={data} resolveFace={resolveFace}/>
                    <BlackHoleStrategyCard title={t("blackhole.debug_result_title")} data={data} resolveFace={resolveFace}/>
                </div>
            </div>
        </div>
    );
}

function ManualSearchabilityCard({data}: { data: PlanData | null }) {
    const {t} = useTranslation();
    if (!data || typeof data.manual_searchable !== "boolean") return null;
    return (
        <section className="panel" style={{marginBottom: 12}}>
            <div className="panel-title">{t("blackhole.debug_searchability_title")}</div>
            <div style={{display: "grid", gap: 8}}>
                <div className={`badge ${data.manual_searchable ? "ok" : "down"}`} style={{width: "fit-content"}}>
                    {data.manual_searchable ? t("blackhole.debug_searchability_yes") : t("blackhole.debug_searchability_no")}
                </div>
                <div className="hint" style={{fontSize: 13}}>{data.manual_search_reason || "-"}</div>
                {Array.isArray(data.component_descs) && data.component_descs.length > 0 ? (
                    <div className="hint" style={{fontSize: 13}}>{data.component_descs.join(" | ")}</div>
                ) : null}
            </div>
        </section>
    );
}

function DebugPoolCard({
    data,
    resolveFace,
}: {
    data: PlanData | null;
    resolveFace: (id: number) => string | null;
}) {
    const {t} = useTranslation();
    const debugPool = data?.debug_pool;
    if (!debugPool) return null;

    const focusFace = debugPool.focus_face || "5s";
    const focusEntries = Array.isArray(debugPool.focus_entries) ? debugPool.focus_entries : [];
    const availableQuads = Array.isArray(debugPool.available_quads) ? debugPool.available_quads : [];
    const focusQuads = availableQuads.filter((item) => item.face === focusFace);
    const normCounts = Object.entries(debugPool.norm_face_counts || {})
        .sort((a, b) => (b[1] || 0) - (a[1] || 0) || a[0].localeCompare(b[0]));

    return (
        <section className="panel" style={{marginBottom: 12}}>
            <div className="panel-title">{t("souzu_debug.pool_diagnostics")}</div>
            <div style={{display: "grid", gap: 14}}>
                <div style={{display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 10}}>
                    <InfoStat label={t("souzu_debug.normalized_count", {face: focusFace})} value={String(debugPool.focus_count ?? 0)}/>
                    <InfoStat label={t("souzu_debug.raw_count", {face: "0s"})} value={String(debugPool.raw_focus_counts?.["0s"] ?? 0)}/>
                    <InfoStat label={t("souzu_debug.raw_count", {face: "5s"})} value={String(debugPool.raw_focus_counts?.["5s"] ?? 0)}/>
                    <InfoStat label={t("souzu_debug.quad_count", {face: focusFace})} value={String(focusQuads.length)}/>
                </div>

                <div style={{display: "grid", gridTemplateColumns: "repeat(4, minmax(0, 1fr))", gap: 10}}>
                    <InfoStat label={t("souzu_debug.pool_hand")} value={String(debugPool.pool_counts?.hand ?? 0)}/>
                    <InfoStat label={t("souzu_debug.pool_replacement")} value={String(debugPool.pool_counts?.replacement_window ?? 0)}/>
                    <InfoStat label={t("souzu_debug.pool_wall")} value={String(debugPool.pool_counts?.wall ?? 0)}/>
                    <InfoStat label={t("souzu_debug.pool_total")} value={String(debugPool.pool_counts?.total ?? 0)}/>
                </div>

                <div style={{display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 10}}>
                    <InfoStat label={t("souzu_debug.used_changes")} value={String(debugPool.replacement_window?.used_count ?? 0)}/>
                    <InfoStat label={t("souzu_debug.remaining_replacement")} value={String(debugPool.replacement_window?.total_remaining ?? 0)}/>
                    <InfoStat label={t("souzu_debug.read_window")} value={String(debugPool.replacement_window?.window_count ?? 0)}/>
                </div>

                <div style={{display: "grid", gap: 8}}>
                    <div style={{fontWeight: 600}}>{t("souzu_debug.focus_details", {face: focusFace})}</div>
                    {focusEntries.length > 0 ? (
                        <DebugTilePositions positions={focusEntries} resolveFace={resolveFace}/>
                    ) : (
                        <div className="hint">{t("souzu_debug.no_focus_tiles", {face: focusFace})}</div>
                    )}
                </div>

                <div style={{display: "grid", gap: 8}}>
                    <div style={{fontWeight: 600}}>{t("souzu_debug.available_quads")}</div>
                    {availableQuads.length > 0 ? (
                        <div style={{display: "grid", gap: 10}}>
                            {availableQuads.map((quad, index) => (
                                <div
                                    key={`${quad.face || "quad"}-${index}`}
                                    style={{
                                        border: "1px solid var(--border)",
                                        borderRadius: 10,
                                        padding: 10,
                                        display: "grid",
                                        gap: 8,
                                        background: quad.face === focusFace ? "color-mix(in oklab, var(--badge-ok-bg) 60%, var(--panel-bg))" : "var(--panel-bg)",
                                    }}
                                >
                                    <div style={{fontWeight: 600}}>{`${index + 1}. ${quad.face || "-"}`}</div>
                                    <DebugTilePositions positions={quad.tile_positions || []} resolveFace={resolveFace}/>
                                </div>
                            ))}
                        </div>
                    ) : (
                        <div className="hint">{t("souzu_debug.no_available_quads")}</div>
                    )}
                </div>

                <div style={{display: "grid", gap: 8}}>
                    <div style={{fontWeight: 600}}>{t("souzu_debug.normalized_counts")}</div>
                    {normCounts.length > 0 ? (
                        <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                            {normCounts.map(([face, count]) => (
                                <span
                                    key={face}
                                    className={`badge ${face === focusFace ? "ok" : ""}`}
                                    style={{fontWeight: face === focusFace ? 800 : 500}}
                                >
                                    {`${face}: ${count}`}
                                </span>
                            ))}
                        </div>
                    ) : (
                        <div className="hint">-</div>
                    )}
                </div>
            </div>
        </section>
    );
}

function InfoStat({label, value}: { label: string; value: string }) {
    return (
        <div
            style={{
                border: "1px solid var(--border)",
                borderRadius: 10,
                padding: "10px 12px",
                display: "grid",
                gap: 6,
            }}
        >
            <div className="hint" style={{fontSize: 12}}>{label}</div>
            <div style={{fontSize: 22, fontWeight: 800, lineHeight: 1.05}}>{value}</div>
        </div>
    );
}

function DebugTilePositions({
    positions,
    resolveFace,
}: {
    positions: Array<{
        tile_id: number;
        source: string;
        source_index: number;
        raw_face?: string;
        norm_face?: string;
    }>;
    resolveFace: (id: number) => string | null;
}) {
    const {t} = useTranslation();
    const sourceLabel = (source: string) => {
        if (source === "hand") return t("souzu_debug.source_hand");
        if (source === "replacement") return t("souzu_debug.source_replacement_window");
        if (source === "wall") return t("souzu_debug.source_wall");
        return source;
    };

    if (!positions.length) {
        return <div className="hint">-</div>;
    }

    return (
        <div style={{display: "flex", flexWrap: "wrap", gap: 10}}>
            {positions.map((item, index) => (
                <div
                    key={`${item.tile_id}-${index}`}
                    style={{
                        border: "1px solid var(--border)",
                        borderRadius: 10,
                        padding: 10,
                        minWidth: 100,
                        display: "grid",
                        gap: 6,
                        justifyItems: "center",
                    }}
                >
                    <Tile tile={(resolveFace(item.tile_id) || item.raw_face || "-")} width={40} height={54}/>
                    <div className="hint" style={{fontSize: 11}}>{`ID ${item.tile_id}`}</div>
                    <div className="hint" style={{fontSize: 11}}>{`${sourceLabel(item.source)} #${item.source_index}`}</div>
                    {"raw_face" in item ? (
                        <div className="hint" style={{fontSize: 11}}>{`${item.raw_face || "-"} -> ${item.norm_face || "-"}`}</div>
                    ) : null}
                </div>
            ))}
        </div>
    );
}

function ManualQuadSelect({
    title,
    value,
    options,
    onChange,
}: {
    title: string;
    value: string;
    options: QuadOption[];
    onChange: (value: string) => void;
}) {
    const {t} = useTranslation();
    return (
        <label style={{display: "grid", gap: 8}}>
            <span>{title}</span>
            <select value={value} onChange={(e) => onChange(e.target.value)}>
                <option value="">{t("souzu_debug.select")}</option>
                {options.map((option) => (
                    <option key={option.key} value={option.key}>{option.label}</option>
                ))}
            </select>
        </label>
    );
}

function ManualBucket({
    title,
    ids,
    resolveFace,
    onRemove,
}: {
    title: string;
    ids: number[];
    resolveFace: (id: number) => string | null;
    onRemove: (tileId: number) => void;
}) {
    const {t} = useTranslation();
    return (
        <div style={{border: "1px solid var(--border)", borderRadius: 12, padding: 12, display: "grid", gap: 10}}>
            <div style={{fontWeight: 700, fontSize: 18}}>{title}</div>
            {ids.length > 0 ? (
                <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                    {ids.map((id, index) => (
                        <button
                            key={`${id}-${index}`}
                            className="nav-btn"
                            onClick={() => onRemove(id)}
                            style={{
                                padding: 6,
                                borderColor: "var(--badge-ok-border)",
                                background: "var(--badge-ok-bg)",
                                boxShadow: "inset 0 0 0 1px var(--badge-ok-border)",
                            }}
                        >
                            <div style={{display: "grid", gap: 4, justifyItems: "center"}}>
                                <Tile tile={resolveFace(id) || "-"} width={40} height={54}/>
                                <div className="hint" style={{color: "var(--text)"}}>{id}</div>
                            </div>
                        </button>
                    ))}
                </div>
            ) : (
                <div className="hint">{t("souzu_debug.click_to_add")}</div>
            )}
        </div>
    );
}

function PoolSection({
    title,
    entries,
    usedQuadIds,
    manualIds,
    onAssign,
}: {
    title: string;
    entries: PoolEntry[];
    usedQuadIds: Set<number>;
    manualIds: Set<number>;
    onAssign: (tileId: number) => void;
}) {
    const {t} = useTranslation();
    return (
        <div style={{display: "grid", gap: 8}}>
            <div style={{display: "flex", justifyContent: "space-between", alignItems: "baseline"}}>
                <div style={{fontWeight: 600}}>{title}</div>
                <div className="hint">{t("souzu_debug.tile_count", {count: entries.length})}</div>
            </div>
            {entries.length > 0 ? (
                <div
                    style={{
                        display: "grid",
                        gridTemplateColumns: "repeat(auto-fill, minmax(88px, 1fr))",
                        gap: 8,
                    }}
                >
                    {entries.map((entry) => {
                        const usedByQuad = usedQuadIds.has(entry.tileId);
                        const usedByManual = manualIds.has(entry.tileId);
                        const used = usedByQuad || usedByManual;
                        const usedLabel = usedByQuad ? t("souzu_debug.used_by_quad") : usedByManual ? t("souzu_debug.assigned") : "";
                        return (
                            <button
                                key={`${entry.source}-${entry.tileId}`}
                                className="nav-btn"
                                onClick={() => onAssign(entry.tileId)}
                                disabled={usedByQuad}
                                style={{
                                    width: "100%",
                                    padding: 6,
                                    minHeight: 116,
                                    display: "grid",
                                    gap: 4,
                                    justifyItems: "center",
                                    alignContent: "start",
                                    borderColor: used ? "var(--toast-error-border)" : "var(--border)",
                                    background: used ? "color-mix(in oklab, var(--danger-weak-bg) 72%, var(--panel-bg))" : "var(--panel-bg)",
                                    boxShadow: used ? "inset 0 0 0 2px var(--toast-error-border)" : "none",
                                    opacity: usedByQuad ? 0.75 : 1,
                                    cursor: usedByQuad ? "not-allowed" : "pointer",
                                }}
                            >
                                <Tile tile={entry.face} width={40} height={54}/>
                                <div className="hint" style={{fontSize: 11}}>
                                    {`${sourceShort(entry.source)}${entry.sourceIndex + 1}`}
                                </div>
                                <div className="hint" style={{fontSize: 11}}>
                                    {entry.tileId}
                                </div>
                                <div
                                    style={{
                                        minHeight: 16,
                                        fontSize: 11,
                                        fontWeight: 700,
                                        color: used ? "var(--toast-error-border)" : "var(--muted)",
                                    }}
                                >
                                    {usedLabel}
                                </div>
                            </button>
                        );
                    })}
                </div>
            ) : (
                <div className="hint">-</div>
            )}
        </div>
    );
}

function SnapshotTileRow({
    title,
    ids,
    resolveFace,
}: {
    title: string;
    ids: number[];
    resolveFace: (id: number) => string | null;
}) {
    const {t} = useTranslation();
    return (
        <div style={{display: "grid", gap: 8}}>
            <div style={{display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline"}}>
                <div style={{fontWeight: 600}}>{title}</div>
                <div className="hint">{t("souzu_debug.tile_count", {count: ids.length})}</div>
            </div>
            {ids.length > 0 ? (
                <div style={{display: "flex", flexWrap: "wrap", gap: 6}}>
                    {ids.map((id, index) => (
                        <div key={`${title}-${id}-${index}`} style={{display: "grid", gap: 4, justifyItems: "center"}}>
                            <Tile tile={resolveFace(id) || "-"} width={40} height={54}/>
                            <div className="hint">ID {id}</div>
                        </div>
                    ))}
                </div>
            ) : (
                <div className="hint">-</div>
            )}
        </div>
    );
}
