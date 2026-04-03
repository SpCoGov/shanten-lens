import React from "react";
import Tile from "../components/Tile";
import {pushToast} from "../lib/toast";
import type {GameStateData} from "../lib/gamestate";
import type {PlanData} from "../lib/planTypes";
import {ws} from "../lib/ws";
import {BlackHoleStrategyCard} from "./BlackHolePage";

export type DebugSnapshot = {
    stage?: number;
    deck_map: Record<string, string>;
    hand_tiles: number[];
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
const STRUCTURE_LIMITS: Record<ManualStructureKey, number> = {
    meld1: 3,
    meld2: 3,
    pair: 2,
};

export function buildDebugSnapshotFromState(state: GameStateData): DebugSnapshot {
    return {
        stage: state.stage,
        deck_map: state.deck_map || {},
        hand_tiles: Array.isArray(state.hand_tiles) ? state.hand_tiles : [],
        replacement_tiles: Array.isArray(state.replacement_tiles) ? state.replacement_tiles : [],
        wall_tiles: Array.isArray(state.wall_tiles) ? state.wall_tiles : [],
        switch_used_tiles: Array.isArray(state.switch_used_tiles) ? state.switch_used_tiles : [],
        total_change_tile_count: state.total_change_tile_count ?? 0,
        change_tile_count: state.change_tile_count ?? 0,
        boss_buff: Array.isArray(state.boss_buff) ? state.boss_buff : [],
    };
}

function safeParseSnapshot(text: string): DebugSnapshot {
    const parsed = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw new Error("导入内容必须是 JSON 对象");
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

function sourceTitle(source: PoolSource) {
    if (source === "hand") return "手牌";
    if (source === "replacement") return "换牌堆";
    return "牌山";
}

function bucketTitle(key: ManualStructureKey) {
    if (key === "meld1") return "面子 A";
    if (key === "meld2") return "面子 B";
    return "雀头";
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
    const replacementTiles = (snapshot.replacement_tiles || []).slice(usedCount);
    const wallTiles = (snapshot.wall_tiles || []).slice(0, wallLimit);
    const entries: PoolEntry[] = [];

    const pushGroup = (source: PoolSource, ids: number[]) => {
        ids.forEach((tileId, sourceIndex) => {
            const face = deckMap[String(tileId)] ?? (deckMap as Record<number, string>)[tileId] ?? "?";
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
    const [snapshotText, setSnapshotText] = React.useState("");
    const [activeSnapshot, setActiveSnapshot] = React.useState<DebugSnapshot | null>(null);
    const [wallLimit, setWallLimit] = React.useState(DEFAULT_WALL_LIMIT);
    const [autoStopFirst, setAutoStopFirst] = React.useState(true);
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
            const parsed = safeParseSnapshot(snapshotText);
            setActiveSnapshot(parsed);
            resetManualBuilder();
            pushToast("局面已导入", "success", 1400);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : "导入失败", "error", 2200);
        }
    }, [resetManualBuilder, snapshotText]);

    const exportCurrent = React.useCallback(async () => {
        if (!currentState) {
            pushToast("当前没有可导出的局面", "error", 1800);
            return;
        }
        const snapshot = buildDebugSnapshotFromState(currentState);
        const nextText = JSON.stringify(snapshot);
        setSnapshotText(nextText);
        setActiveSnapshot(snapshot);
        resetManualBuilder();
        try {
            await navigator.clipboard.writeText(nextText);
            pushToast("已导出当前局面，并复制到剪贴板", "success", 1600);
        } catch {
            pushToast("已导出当前局面", "success", 1400);
        }
    }, [currentState, resetManualBuilder]);

    const runImportedSnapshot = React.useCallback(() => {
        let snapshot: DebugSnapshot;
        try {
            snapshot = safeParseSnapshot(snapshotText);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : "导入内容无效", "error", 2200);
            return;
        }
        setActiveSnapshot(snapshot);
        ws.send({
            type: "souzu_switch_control",
            data: {
                action: "start_debug",
                snapshot,
                options: {
                    stop_after_first: autoStopFirst,
                    skip_signatures: [],
                    wall_limit: wallLimit,
                },
            },
        } as any);
    }, [autoStopFirst, snapshotText, wallLimit]);

    const validateManualPlan = React.useCallback(() => {
        let snapshot: DebugSnapshot;
        try {
            snapshot = activeSnapshot ?? safeParseSnapshot(snapshotText);
        } catch (error) {
            pushToast(error instanceof Error ? error.message : "请先导入局面", "error", 2200);
            return;
        }
        if (!selectedQuads[0] || !selectedQuads[1]) {
            pushToast("请先选择两组杠", "error", 1800);
            return;
        }
        if (selectedQuads[0].key === selectedQuads[1].key) {
            pushToast("两组杠不能相同", "error", 1800);
            return;
        }
        const totalTiles = manualStructure.meld1.length + manualStructure.meld2.length + manualStructure.pair.length;
        if (totalTiles !== 7) {
            pushToast("听牌前目标一共需要 7 张牌", "error", 1800);
            return;
        }
        ws.send({
            type: "souzu_switch_control",
            data: {
                action: "validate_manual_debug",
                snapshot,
                quad_groups: [selectedQuads[0].ids, selectedQuads[1].ids],
                structure_groups: manualStructure,
                options: {wall_limit: wallLimit},
            },
        } as any);
    }, [activeSnapshot, manualStructure, selectedQuads, snapshotText, wallLimit]);

    const clearDebugState = React.useCallback(() => {
        setSnapshotText("");
        setActiveSnapshot(null);
        resetManualBuilder();
        onClear?.();
    }, [onClear, resetManualBuilder]);

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
                pushToast("当前分组已满", "error", 1400);
                return prev;
            }
            next[activeBucket].push(tileId);
            return next;
        });
    }, [activeBucket, selectedQuadIds]);

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
                <div className="panel-title">Souzu Switch 调试工具</div>
                <div style={{display: "flex", flexWrap: "wrap", gap: 12, alignItems: "center"}}>
                    <button className="nav-btn" onClick={exportCurrent}>导出当前局面</button>
                    <button className="nav-btn" onClick={importSnapshot}>导入文本局面</button>
                    <button className="nav-btn" onClick={runImportedSnapshot}>对导入局面求解</button>
                    <button className="nav-btn" onClick={validateManualPlan}>验证手工方案</button>
                    <button className="nav-btn" onClick={clearDebugState}>清空调试数据</button>
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <input type="checkbox" checked={autoStopFirst} onChange={(e) => setAutoStopFirst(e.target.checked)}/>
                        <span>找到第一套方案后停止</span>
                    </label>
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <span>牌山读取上限</span>
                        <input
                            className="form-input"
                            style={{width: 88}}
                            type="number"
                            min={2}
                            max={36}
                            value={wallLimit}
                            onChange={(e) => setWallLimit(Math.min(36, Math.max(2, Number(e.target.value) || 2)))}
                        />
                    </label>
                </div>
                <div style={{marginTop: 10, color: "var(--muted)", fontSize: 13}}>
                    手工验证模式按你指定的两组杠和 7 张目标牌做验证，并额外说明搜索器能否自己枚举到该方案。
                </div>
            </div>

            <div className="responsive-two-col">
                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">局面 JSON</div>
                    <textarea
                        className="form-input"
                        style={{minHeight: 300, maxWidth: "none", fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"}}
                        value={snapshotText}
                        onChange={(e) => setSnapshotText(e.target.value)}
                        placeholder='{"stage":2,"deck_map":{},"hand_tiles":[]}'
                    />
                </section>

                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">已导入局面预览</div>
                    {activeSnapshot ? (
                        <div style={{display: "grid", gap: 12}}>
                            <SnapshotTileRow title="手牌" ids={activeSnapshot.hand_tiles ?? []} resolveFace={resolveFace}/>
                            <SnapshotTileRow title="换牌堆" ids={activeSnapshot.replacement_tiles ?? []} resolveFace={resolveFace}/>
                            <SnapshotTileRow title="牌山" ids={(activeSnapshot.wall_tiles ?? []).slice(0, wallLimit)} resolveFace={resolveFace}/>
                            <div style={{display: "flex", flexWrap: "wrap", gap: 10}}>
                                <span className="badge">阶段: {String(activeSnapshot.stage ?? "-")}</span>
                                <span className="badge">已换: {String(activeSnapshot.change_tile_count ?? 0)}</span>
                                <span className="badge">总换牌次数: {String(activeSnapshot.total_change_tile_count ?? 0)}</span>
                                <span className="badge">Boss Buff: {(activeSnapshot.boss_buff ?? []).join(", ") || "-"}</span>
                            </div>
                        </div>
                    ) : (
                        <div className="hint">还没有导入局面。可以先导出当前局面，或直接粘贴 JSON。</div>
                    )}
                </section>
            </div>

            {activeSnapshot && (
                <section className="panel" style={{minWidth: 0}}>
                    <div className="panel-title">手工方案验证</div>
                    <div style={{display: "grid", gap: 16}}>
                        <div style={{display: "grid", gridTemplateColumns: "repeat(2, minmax(0, 1fr))", gap: 12}}>
                            <ManualQuadSelect
                                title="杠 A"
                                value={selectedQuadKeys[0]}
                                options={quadOptions}
                                onChange={(value) => setSelectedQuadKeys(([_, second]) => [value, second])}
                            />
                            <ManualQuadSelect
                                title="杠 B"
                                value={selectedQuadKeys[1]}
                                options={quadOptions}
                                onChange={(value) => setSelectedQuadKeys(([first]) => [first, value])}
                            />
                        </div>

                        <div style={{display: "grid", gap: 10}}>
                            <div className="hint">当前点击分配到</div>
                            <div style={{display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center"}}>
                                {(["meld1", "meld2", "pair"] as ManualStructureKey[]).map((key) => (
                                    <button
                                        key={key}
                                        className={`nav-btn ${activeBucket === key ? "active" : ""}`}
                                        onClick={() => setActiveBucket(key)}
                                    >
                                        {bucketTitle(key)} {manualStructure[key].length}/{STRUCTURE_LIMITS[key]}
                                    </button>
                                ))}
                                <button className="nav-btn" onClick={resetManualBuilder}>重置分组</button>
                                <span className="badge">{`当前总数: ${totalTiles}/7`}</span>
                            </div>
                        </div>

                        <div style={{display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 12}}>
                            {(["meld1", "meld2", "pair"] as ManualStructureKey[]).map((key) => (
                                <ManualBucket
                                    key={key}
                                    title={bucketTitle(key)}
                                    ids={manualStructure[key]}
                                    resolveFace={resolveFace}
                                    onRemove={(tileId) => removeTile(key, tileId)}
                                />
                            ))}
                        </div>

                        <div style={{display: "grid", gap: 12}}>
                            <div className="hint">可分配的牌</div>
                            {(["hand", "replacement", "wall"] as PoolSource[]).map((source) => (
                                <PoolSection
                                    key={source}
                                    title={sourceTitle(source)}
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
                    <BlackHoleStrategyCard title="调试结果" data={data} resolveFace={resolveFace}/>
                </div>
            </div>
        </div>
    );
}

function ManualSearchabilityCard({data}: { data: PlanData | null }) {
    if (!data || typeof data.manual_searchable !== "boolean") return null;
    return (
        <section className="panel" style={{marginBottom: 12}}>
            <div className="panel-title">搜索器可发现性</div>
            <div style={{display: "grid", gap: 8}}>
                <div className={`badge ${data.manual_searchable ? "ok" : "down"}`} style={{width: "fit-content"}}>
                    {data.manual_searchable ? "搜索器可以枚举到这套方案" : "搜索器当前枚举不到这套方案"}
                </div>
                <div className="hint" style={{fontSize: 13}}>{data.manual_search_reason || "-"}</div>
                {Array.isArray(data.component_descs) && data.component_descs.length > 0 ? (
                    <div className="hint" style={{fontSize: 13}}>{data.component_descs.join(" | ")}</div>
                ) : null}
            </div>
        </section>
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
    return (
        <label style={{display: "grid", gap: 8}}>
            <span>{title}</span>
            <select value={value} onChange={(e) => onChange(e.target.value)}>
                <option value="">请选择</option>
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
                <div className="hint">点击下方牌加入当前分组</div>
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
    return (
        <div style={{display: "grid", gap: 8}}>
            <div style={{display: "flex", justifyContent: "space-between", alignItems: "baseline"}}>
                <div style={{fontWeight: 600}}>{title}</div>
                <div className="hint">{entries.length} 张</div>
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
                        const usedLabel = usedByQuad ? "已用于杠" : usedByManual ? "已分配" : "";
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
    return (
        <div style={{display: "grid", gap: 8}}>
            <div style={{display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline"}}>
                <div style={{fontWeight: 600}}>{title}</div>
                <div className="hint">{ids.length} 张</div>
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
