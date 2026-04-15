import React from "react";
import {useTranslation} from "react-i18next";
import {pushToast} from "../lib/toast";
import {ws} from "../lib/ws";
import Tile from "../components/Tile";
import styles from "../components/AdvisorPanel.module.css";
import type {PlanData, SearchRuntimeData, TileId} from "../lib/planTypes";
import type {GameStateData} from "../lib/gamestate";
import {buildDebugSnapshotFromState} from "./SouzuSwitchDebugPage";

const LS_AUTO_STOP = "sl-blackhole:auto-stop-first";
const LS_VERBOSE = "sl-blackhole:verbose-progress";
const LS_WALL_LIMIT = "sl-blackhole:wall-limit";
const LS_SEARCH_ALGORITHM = "sl-blackhole:search-algorithm";
const WALL_LIMIT_MIN = 2;
const WALL_LIMIT_MAX = 36;
const DEFAULT_SEARCH_ALGORITHM = "constraint_decomposition_dfs";

function readSearchAlgorithm() {
    const raw = localStorage.getItem(LS_SEARCH_ALGORITHM) || DEFAULT_SEARCH_ALGORITHM;
    return raw === "target_enumeration_search" ? "target_enumeration_search" : DEFAULT_SEARCH_ALGORITHM;
}

function readBool(key: string, fallback: boolean) {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return raw === "1";
}

function readWallLimit() {
    const raw = Number(localStorage.getItem(LS_WALL_LIMIT) || "36");
    if (!Number.isFinite(raw)) return 36;
    return Math.min(36, Math.max(2, Math.trunc(raw)));
}

function parseWallLimitInput(raw: string) {
    const value = Number(raw);
    if (!Number.isFinite(value) || !Number.isInteger(value)) {
        return {ok: false as const, message: "牌山读取上限必须是 2 到 36 的整数"};
    }
    if (value < WALL_LIMIT_MIN || value > WALL_LIMIT_MAX) {
        return {ok: false as const, message: "牌山读取上限必须在 2 到 36 之间"};
    }
    return {ok: true as const, value};
}

function summarizeProgress(progress?: string) {
    if (!progress) return "";
    const lines = progress
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line && !line.includes("剩余换牌") && !line.includes("当前阶段"));
    const keepPrefixes = [
        "已耗时",
        "已完成搜索次数",
        "搜索速度",
        "当前双杠序号",
        "已找到候选方案",
        "当前最新结果",
    ];
    const summary = lines.filter((line, index) => {
        if (index === 0) return true;
        return keepPrefixes.some((prefix) => line.startsWith(prefix));
    });
    return summary.join("\n");
}

function visibleProgress(progress?: string) {
    if (!progress) return "";
    return progress
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line && !line.includes("剩余换牌") && !line.includes("当前阶段"))
        .join("\n");
}

export default function BlackHolePage({
                                          stage,
                                          data,
                                          resolveFace,
                                          handIds,
                                          replacementIds,
                                          wallIds,
                                          currentState,
                                          runtime,
                                          onClear,
                                      }: {
    stage: number;
    data: PlanData | null;
    resolveFace?: (id: number) => string | null;
    handIds: TileId[];
    replacementIds: TileId[];
    wallIds: TileId[];
    currentState: GameStateData | null;
    runtime?: SearchRuntimeData | null;
    onClear?: () => void;
}) {
    const {t} = useTranslation();
    const [autoStopFirst, setAutoStopFirst] = React.useState<boolean>(() => readBool(LS_AUTO_STOP, true));
    const [verboseProgress, setVerboseProgress] = React.useState<boolean>(() => readBool(LS_VERBOSE, false));
    const [wallLimit, setWallLimit] = React.useState<number>(() => readWallLimit());
    const [wallLimitInput, setWallLimitInput] = React.useState<string>(() => String(readWallLimit()));
    const [searchAlgorithm, setSearchAlgorithm] = React.useState<string>(() => readSearchAlgorithm());
    const [seenSignatures, setSeenSignatures] = React.useState<string[]>([]);
    const [mainData, setMainData] = React.useState<PlanData | null>(null);
    const [quadCatalogData, setQuadCatalogData] = React.useState<PlanData | null>(null);
    const [quadDrawerOpen, setQuadDrawerOpen] = React.useState(false);
    const [drawerMode, setDrawerMode] = React.useState<"quad" | "considered">("quad");

    React.useEffect(() => localStorage.setItem(LS_AUTO_STOP, autoStopFirst ? "1" : "0"), [autoStopFirst]);
    React.useEffect(() => localStorage.setItem(LS_VERBOSE, verboseProgress ? "1" : "0"), [verboseProgress]);
    React.useEffect(() => localStorage.setItem(LS_WALL_LIMIT, String(wallLimit)), [wallLimit]);
    React.useEffect(() => localStorage.setItem(LS_SEARCH_ALGORITHM, searchAlgorithm), [searchAlgorithm]);
    React.useEffect(() => {
        if (!data) return;
        if ((data as any).status === "catalog") {
            setQuadCatalogData(data);
            setDrawerMode("quad");
            setQuadDrawerOpen(true);
            return;
        }
        setMainData(data);
    }, [data]);
    React.useEffect(() => {
        if (!mainData?.plan_signature) return;
        setSeenSignatures((prev) => prev.includes(mainData.plan_signature!) ? prev : [...prev, mainData.plan_signature!]);
    }, [mainData?.plan_signature]);
    React.useEffect(() => {
        if (stage === 2) return;
        setSeenSignatures([]);
    }, [stage]);
    React.useEffect(() => {
        if (stage === 2) return;
        if (mainData?.status === "searching") {
            ws.send({type: "souzu_switch_control", data: {action: "stop", notify: false}} as any);
        }
    }, [stage, mainData?.status]);

    const planSignature = mainData?.plan_signature || "";
    const canOperate = stage === 2;
    const isSearching = mainData?.status === "searching";
    const canResume = !!planSignature && !isSearching;
    const hasExecutablePlan = !!(
        mainData &&
        mainData.status === "plan" &&
        Array.isArray(mainData.switch_discards) &&
        mainData.switch_discards.length > 0
    );

    const resolveWallLimit = React.useCallback(() => {
        const parsed = parseWallLimitInput(wallLimitInput.trim());
        if (!parsed.ok) {
            pushToast(parsed.message, "error", 2200);
            return null;
        }
        setWallLimit(parsed.value);
        setWallLimitInput(String(parsed.value));
        return parsed.value;
    }, [wallLimitInput]);

    const startSearch = React.useCallback((opts?: { stopAfterFirst?: boolean; resume?: boolean }) => {
        if (!canOperate) {
            pushToast(t("blackhole.need_switch_stage"), "error", 1800);
            return;
        }
        const nextWallLimit = resolveWallLimit();
        if (nextWallLimit == null) return;
        const skipSignatures = opts?.resume
            ? Array.from(new Set([...seenSignatures, ...(planSignature ? [planSignature] : [])]))
            : [];
        if (!opts?.resume) {
            setSeenSignatures([]);
        }
        ws.send({
            type: "souzu_switch_control",
            data: {
                action: "start",
                options: {
                    stop_after_first: opts?.stopAfterFirst ?? autoStopFirst,
                    skip_signatures: skipSignatures,
                    wall_limit: nextWallLimit,
                    search_algorithm: searchAlgorithm,
                },
            },
        } as any);
    }, [autoStopFirst, canOperate, planSignature, resolveWallLimit, searchAlgorithm, seenSignatures, t]);

    const stopSearch = React.useCallback(() => {
        ws.send({type: "souzu_switch_control", data: {action: "stop"}} as any);
    }, []);
    const refreshRuntime = React.useCallback(() => {
        ws.send({type: "souzu_switch_control", data: {action: "runtime_status"}} as any);
    }, []);
    const killWorkers = React.useCallback(() => {
        ws.send({type: "souzu_switch_control", data: {action: "kill_workers"}} as any);
        ws.send({type: "souzu_switch_control", data: {action: "runtime_status"}} as any);
    }, []);
    const listQuads = React.useCallback(() => {
        const nextWallLimit = resolveWallLimit();
        if (nextWallLimit == null) return;
        setDrawerMode("quad");
        setQuadDrawerOpen(true);
        ws.send({
            type: "souzu_switch_control",
            data: {action: "list_quads", options: {wall_limit: nextWallLimit, search_algorithm: searchAlgorithm}},
        } as any);
    }, [resolveWallLimit, searchAlgorithm]);
    const openConsideredTiles = React.useCallback(() => {
        setDrawerMode("considered");
        setQuadDrawerOpen(true);
    }, []);
    const executePlan = React.useCallback(() => {
        if (!canOperate || !hasExecutablePlan) return;
        ws.send({type: "souzu_switch_control", data: {action: "execute_plan"}} as any);
    }, [canOperate, hasExecutablePlan]);
    const exportCurrentSnapshot = React.useCallback(async () => {
        if (!currentState) {
            pushToast("当前没有可导出的局面", "error", 1800);
            return;
        }
        const snapshot = buildDebugSnapshotFromState(currentState);
        const text = JSON.stringify(snapshot);
        try {
            await navigator.clipboard.writeText(text);
            pushToast("已导出当前局面，并复制到剪贴板", "success", 1600);
        } catch {
            pushToast("已导出当前局面", "success", 1400);
        }
    }, [currentState]);
    const clearCache = React.useCallback(() => {
        setSeenSignatures([]);
        setQuadCatalogData(null);
        setQuadDrawerOpen(false);
        setMainData(null);
        onClear?.();
    }, [onClear]);

    const viewData = React.useMemo(() => {
        if (!mainData || mainData.status !== "searching") return mainData;
        if (verboseProgress) {
            return {
                ...mainData,
                progress: visibleProgress(mainData.progress),
            };
        }
        return {
            ...mainData,
            progress: summarizeProgress(mainData.progress),
        };
    }, [mainData, verboseProgress]);
    const consideredWallIds = React.useMemo(
        () => (wallLimit >= 36 ? wallIds : wallIds.slice(0, Math.max(2, wallLimit))),
        [wallIds, wallLimit],
    );
    const consideredSections = React.useMemo(() => ([
        {key: "hand", title: "手牌", ids: handIds},
        {key: "replacement", title: "换牌堆", ids: replacementIds},
        {key: "wall", title: "牌山", ids: consideredWallIds},
    ]), [consideredWallIds, handIds, replacementIds]);

    return (
        <div className="settings-wrap wide-page" style={{paddingBlock: 16}}>
            <div className="panel">
                <div className="panel-title">{t("blackhole.title")}</div>
                <div style={{display: "flex", flexWrap: "wrap", gap: 12, alignItems: "center"}}>
                    <button className="nav-btn" onClick={() => startSearch()} disabled={isSearching}>
                        {t("blackhole.start")}
                    </button>
                    <button className="nav-btn" onClick={stopSearch} disabled={!isSearching}>
                        {t("blackhole.stop")}
                    </button>
                    <button className="nav-btn" onClick={killWorkers}>
                        {"强制清理子进程"}
                    </button>
                    <button className="nav-btn" onClick={refreshRuntime}>
                        {"刷新子进程"}
                    </button>
                    <button className="nav-btn" onClick={() => startSearch({stopAfterFirst: false, resume: true})} disabled={!canResume}>
                        {t("blackhole.continue")}
                    </button>
                    <button className="nav-btn" onClick={() => startSearch({stopAfterFirst: true, resume: true})} disabled={!canResume}>
                        {t("blackhole.next")}
                    </button>
                    <button className="nav-btn" onClick={listQuads}>
                        {t("blackhole.list_quads")}
                    </button>
                    <button className="nav-btn" onClick={executePlan} disabled={!canOperate || !hasExecutablePlan || isSearching}>
                        {t("blackhole.execute_plan")}
                    </button>
                    <button className="nav-btn" onClick={exportCurrentSnapshot}>
                        {"导出当前局面"}
                    </button>
                    <button className="nav-btn" onClick={clearCache}>
                        {t("blackhole.clear_cache")}
                    </button>

                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <input type="checkbox" checked={autoStopFirst} onChange={(e) => setAutoStopFirst(e.target.checked)}/>
                        <span>{t("blackhole.auto_stop_first")}</span>
                    </label>
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <input type="checkbox" checked={verboseProgress} onChange={(e) => setVerboseProgress(e.target.checked)}/>
                        <span>{t("blackhole.verbose_progress")}</span>
                    </label>
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
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                        <span>{t("blackhole.search_algorithm")}</span>
                        <select
                            className="form-input"
                            value={searchAlgorithm}
                            onChange={(e) => setSearchAlgorithm(e.target.value)}
                        >
                            <option value="constraint_decomposition_dfs">
                                {t("blackhole.algorithm_constraint_dfs")}
                            </option>
                            <option value="target_enumeration_search">
                                {t("blackhole.algorithm_target_enum")}
                            </option>
                        </select>
                    </label>
                </div>
                <div style={{marginTop: 10, color: "var(--muted)", fontSize: 13}}>
                    {canOperate ? t("blackhole.stage_ready") : t("blackhole.stage_not_ready")} {t("blackhole.wall_limit_hint")}
                </div>
            </div>

            <div className={`blackhole-layout ${quadDrawerOpen ? "with-drawer" : ""}`}>
                <div className="blackhole-main">
                    <SouzuRuntimePanel runtime={runtime}/>
                    <BlackHoleStrategyCard
                        data={viewData}
                        runtime={runtime}
                        resolveFace={resolveFace}
                        onOpenConsideredTiles={openConsideredTiles}
                        title={t("blackhole.card_title")}
                    />
                </div>

                {quadDrawerOpen && (
                    <aside className="blackhole-drawer panel">
                        <div className="blackhole-drawer-head">
                            <div className="panel-title" style={{marginBottom: 0}}>
                                {drawerMode === "quad" ? "所有可见杠" : "可使用的牌"}
                            </div>
                            <button className="nav-btn" onClick={() => setQuadDrawerOpen(false)}>
                                {t("modal.close")}
                            </button>
                        </div>
                        {drawerMode === "quad" ? (
                            <QuadCatalogBody data={quadCatalogData} resolveFace={resolveFace}/>
                        ) : (
                            <ConsideredTilesBody sections={consideredSections} resolveFace={resolveFace}/>
                        )}
                    </aside>
                )}
            </div>
        </div>
    );
}

export function BlackHoleStrategyCard({
                                          title,
                                          data,
                                          runtime,
                                          resolveFace,
                                          onOpenConsideredTiles,
                                      }: {
    title: string;
    data: PlanData | null;
    runtime?: SearchRuntimeData | null;
    resolveFace?: (id: number) => string | null;
    onOpenConsideredTiles?: () => void;
}) {
    const {t} = useTranslation();
    const hasPlanPreview = !!(
        data &&
        (
            (data.switch_discards && data.switch_discards.length > 0) ||
            (data.switch_in && data.switch_in.length > 0) ||
            (data.quad_faces && data.quad_faces.length > 0) ||
            (data.waits && data.waits.length > 0) ||
            (data.wall_draws && data.wall_draws.length > 0)
        )
    );
    const badgeText = data?.status === "plan"
        ? t("advisor.badge_switch_plan")
        : data?.status === "searching" && hasPlanPreview
            ? t("advisor.badge_switch_plan")
            : "";
    return (
        <section className={styles.card}>
            <div className={styles.cardHead}>
                <h3>{title}</h3>
                {badgeText ? <span className={styles.badgeGreen}>{badgeText}</span> : null}
            </div>
            {!data ? (
                <div className={styles.cardBodyMuted}>{t("advisor.awaiting_backend")}</div>
            ) : data.status === "catalog" ? (
                <QuadCatalogBody data={data} resolveFace={resolveFace}/>
            ) : data.status === "searching" ? (
                <div style={{display: "grid", gap: 12}}>
                    <div className={styles.cardBodyMuted} style={{whiteSpace: "pre-wrap"}}>
                        {data.progress || t("advisor.searching")}
                    </div>
                    <div style={{padding: "0 12px"}}>
                        <WorkerStatusPanel data={data} runtime={runtime}/>
                    </div>
                    {!hasPlanPreview ? <div style={{padding: "0 12px 12px"}}><SearchParamsBand data={data} onOpenConsideredTiles={onOpenConsideredTiles}/></div> : null}
                    {hasPlanPreview ? <PlanBody data={data} resolveFace={resolveFace} onOpenConsideredTiles={onOpenConsideredTiles}/> : null}
                </div>
            ) : data.status === "impossible" ? (
                <div style={{padding: 12, display: "grid", gap: 12}}>
                    <WorkerStatusPanel data={data} runtime={runtime}/>
                    <SearchParamsBand data={data} onOpenConsideredTiles={onOpenConsideredTiles}/>
                    <div className={styles.bandSingle} style={{padding: 0}}>
                        <div className={styles.bandValue}>{t("advisor.impossible")}</div>
                        <div className={styles.cardBodyMuted} style={{padding: 0}}>{reasonText(t, data.reason)}</div>
                    </div>
                </div>
            ) : data.status === "plan" ? (
                <div style={{display: "grid", gap: 12}}>
                    <div style={{padding: "12px 12px 0"}}>
                        <WorkerStatusPanel data={data} runtime={runtime}/>
                    </div>
                    <PlanBody data={data} resolveFace={resolveFace} onOpenConsideredTiles={onOpenConsideredTiles}/>
                </div>
            ) : (
                <div className={styles.cardBodyMuted}>{t("advisor.awaiting_backend")}</div>
            )}
        </section>
    );
}

function PlanBody({
                      data,
                      resolveFace,
                      onOpenConsideredTiles,
                  }: {
    data: PlanData;
    resolveFace?: (id: number) => string | null;
    onOpenConsideredTiles?: () => void;
}) {
    const {t} = useTranslation();
    const batches = zipBatches(data.switch_discards, data.switch_in, data.switch_batch_sizes);
    return (
        <div style={{padding: 12, display: "grid", gap: 14}}>
            <SearchParamsBand data={data} onOpenConsideredTiles={onOpenConsideredTiles}/>
            <div className={styles.band}>
                <div className={styles.bandLeft} style={{gridColumn: "1 / -1"}}>
                    <div className={styles.bandActionLabel}>{"第几张听牌"}</div>
                    <div className={styles.bandValue}>{String(data.draws_needed ?? "-")}</div>
                </div>
            </div>

            {batches.map((batch, index) => (
                <div key={index} className="blackhole-section">
                    <details>
                        <summary style={{cursor: "pointer", listStylePosition: "inside"}}>
                            <span className={styles.label}>{t("advisor.switch_batch_title", {index: index + 1})}</span>
                            <span className={styles.bandLabel} style={{marginLeft: 8}}>{t("advisor.batch_size", {count: batch.size})}</span>
                        </summary>
                        <div style={{display: "grid", gap: 14, marginTop: 10}}>
                            <TileGroup title={t("advisor.switch_batch_out", {index: index + 1})} ids={batch.out} resolveFace={resolveFace}/>
                            <TileGroup title={t("advisor.switch_batch_in", {index: index + 1})} ids={batch.in} resolveFace={resolveFace}/>
                        </div>
                    </details>
                </div>
            ))}

            <FinalShapeGroup quadFaces={data.quad_faces || []} tenpaiFaces={data.target13 || []}/>
            <TileGroup title={t("advisor.wall_draw_sequence")} ids={data.wall_draws || []} resolveFace={resolveFace}/>
            <TileGroup title={t("advisor.post_draw_discards")} ids={data.post_draw_discards || []} resolveFace={resolveFace}/>
            <FaceChipList title={t("advisor.final_waits")} faces={data.waits || []}/>
        </div>
    );
}

function SearchParamsBand({
                              data,
                              onOpenConsideredTiles,
                          }: {
    data: PlanData;
    onOpenConsideredTiles?: () => void;
}) {
    if (
        typeof data.max_change_count !== "number" &&
        typeof data.per_change_limit !== "number" &&
        typeof data.considered_tile_count !== "number"
    ) {
        return null;
    }
    const perChangeLimit = typeof data.per_change_limit === "number"
        ? (data.per_change_limit === 13 ? "无限制" : String(data.per_change_limit))
        : "-";
    const items = [
        {label: "最大换牌次数", value: String(data.max_change_count ?? "-"), clickable: false},
        {label: "换牌限制", value: perChangeLimit, clickable: false},
        {
            label: "可使用的牌数",
            value: String(data.considered_tile_count ?? "-"),
            clickable: typeof data.considered_tile_count === "number" && !!onOpenConsideredTiles,
        },
    ];

    return (
        <div style={{display: "grid", gap: 8}}>
            <div className={styles.label}>{"当前参数"}</div>
            <div style={{display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 10}}>
                {items.map((item) => (
                    <div
                        key={item.label}
                        onClick={item.clickable ? onOpenConsideredTiles : undefined}
                        style={{
                            border: "1px solid var(--color-divider)",
                            borderRadius: 10,
                            padding: "10px 12px",
                            display: "grid",
                            gap: 6,
                            minWidth: 0,
                            cursor: item.clickable ? "pointer" : "default",
                        }}
                    >
                        <div className={styles.bandLabel}>{item.label}</div>
                        <div style={{fontSize: 20, fontWeight: 800, lineHeight: 1.05, wordBreak: "break-word"}}>
                            {item.value}
                        </div>
                        {item.clickable ? (
                            <div className={styles.cardBodyMuted} style={{padding: 0}}>{"点击查看牌面"}</div>
                        ) : null}
                    </div>
                ))}
            </div>
        </div>
    );
}

export function SouzuRuntimePanel({runtime}: { runtime?: SearchRuntimeData | null }) {
    if (!runtime) return null;
    const processes = runtime.processes || [];
    return (
        <section className="panel" style={{marginBottom: 12}}>
            <div className="panel-title">搜索运行时</div>
            <div style={{display: "grid", gap: 8}}>
                <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                    <span className="badge">{`状态: ${runtime.searching ? "搜索中" : "空闲"}`}</span>
                    <span className="badge">{`子进程: ${runtime.process_count ?? processes.length}`}</span>
                    <span className="badge">{`更新时间: ${runtime.updated_at ? new Date(runtime.updated_at * 1000).toLocaleTimeString() : "-"}`}</span>
                </div>
                {processes.length > 0 ? (
                    <div style={{display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 10}}>
                        {processes.map((process, index) => (
                            <div key={`${process.pid ?? "proc"}-${index}`} style={{border: "1px solid var(--border)", borderRadius: 10, padding: 10, display: "grid", gap: 6}}>
                                <div style={{display: "flex", justifyContent: "space-between", gap: 8}}>
                                    <div style={{fontWeight: 700}}>{`子进程 ${index + 1}`}</div>
                                    <div className="hint">{process.status || "-"}</div>
                                </div>
                                <div className="hint">{`PID: ${process.pid ?? "-"}`}</div>
                                <div className="hint">{`存活: ${process.alive ? "是" : "否"}`}</div>
                                <div className="hint">{`退出码: ${process.exitcode ?? "-"}`}</div>
                            </div>
                        ))}
                    </div>
                ) : (
                    <div className="hint">{"当前没有活动中的搜索子进程。"}</div>
                )}
            </div>
        </section>
    );
}

export function WorkerStatusPanel({data, runtime}: { data: PlanData; runtime?: SearchRuntimeData | null }) {
    const workers = data.worker_states || [];
    const parallel = data.parallel_info;
    const processes = runtime?.processes || data.runtime?.processes || [];
    if (!workers.length && !parallel && !processes.length) return null;
    const fallbackReason = parallel?.fallback_reason?.trim();
    const disabledReason = parallel?.disabled_reason?.trim();
    const startError = parallel?.start_error?.trim();

    const statusText = (status?: string) => {
        if (status === "running") return "运行中";
        if (status === "completed") return "刚完成";
        if (status === "done") return "已结束";
        if (status === "stopped") return "已停止";
        if (status === "idle") return "空闲";
        return status || "-";
    };

    const describeParallelError = (message?: string | null) => {
        const text = (message || "").trim();
        if (!text) return "";
        if (text.includes("'NoneType' object has no attribute 'get'")) {
            return "子进程返回了空结果，主进程读取结果字段时失败，详细堆栈已输出到后端控制台。";
        }
        return "子进程启动或执行过程中出现异常，详细堆栈已输出到后端控制台。";
    };

    const startErrorText = describeParallelError(startError);
    const fallbackText = describeParallelError(fallbackReason);

    return (
        <div style={{display: "grid", gap: 8}}>
            <div className={styles.label}>{"搜索进程"}</div>
            <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                <span className="badge">{`模式: ${parallel?.enabled ? "多进程" : "单进程"}`}</span>
                <span className="badge">{`并行数: ${parallel?.max_workers ?? (workers.length || 1)}`}</span>
                <span className="badge">{`已完成: ${parallel?.completed_jobs ?? 0}/${parallel?.total_jobs ?? 0}`}</span>
                <span className="badge">{`真实子进程: ${runtime?.process_count ?? data.runtime?.process_count ?? processes.length}`}</span>
            </div>
            <div className="hint" style={{whiteSpace: "pre-wrap", wordBreak: "break-word"}}>
                {`并行判定: CPU核心数=${parallel?.cpu_count ?? "-"}，配置上限=${parallel?.configured_max_workers ?? "-"}，双杠候选=${parallel?.quad_pair_count ?? "-"}，满足并行条件=${parallel?.parallel_ok ? "是" : "否"}，已尝试启动子进程=${parallel?.attempted ? "是" : "否"}`}
            </div>
            {disabledReason ? (
                <div className="hint" style={{whiteSpace: "pre-wrap", wordBreak: "break-word"}}>
                    {`未启动子进程原因: ${disabledReason}`}
                </div>
            ) : null}
            {startErrorText ? (
                <div className="hint" style={{whiteSpace: "pre-wrap", wordBreak: "break-word"}}>
                    {`子进程启动失败: ${startErrorText}`}
                </div>
            ) : null}
            {fallbackText ? (
                <div className="hint" style={{whiteSpace: "pre-wrap", wordBreak: "break-word"}}>
                    {`已回退为单进程搜索: ${fallbackText}`}
                </div>
            ) : null}
            <div style={{display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))", gap: 10}}>
                {workers.map((worker, index) => (
                    <div
                        key={`${worker.worker_id || index}-${worker.current_quad_index || 0}`}
                        style={{
                            border: "1px solid var(--border)",
                            borderRadius: 10,
                            padding: 10,
                            display: "grid",
                            gap: 6,
                        }}
                    >
                        <div style={{display: "flex", justifyContent: "space-between", gap: 8}}>
                            <div style={{fontWeight: 700}}>{`${worker.kind === "process" ? "进程" : "主搜索"} ${worker.worker_id ?? index + 1}`}</div>
                            <div className="hint">{statusText(worker.status)}</div>
                        </div>
                        <div className="hint">{worker.current_quad_index ? `负责双杠 #${worker.current_quad_index}` : "当前未分配任务"}</div>
                        <div className="hint" style={{whiteSpace: "pre-wrap", wordBreak: "break-word"}}>{worker.current_quad_label || "-"}</div>
                        <div className="hint">{`已完成任务: ${worker.completed_jobs ?? 0}`}</div>
                        <div className="hint">{`耗时: ${(worker.elapsed_sec ?? 0).toFixed(2)} 秒`}</div>
                        <div className="hint">{`最近结果: ${worker.last_result || "-"}`}</div>
                        <div className="hint">{`最近摸牌数: ${worker.last_draws_needed ?? "-"}`}</div>
                    </div>
                ))}
            </div>
        </div>
    );
}

function ConsideredTilesBody({
                                 sections,
                                 resolveFace,
                             }: {
    sections: Array<{ key: string; title: string; ids: TileId[] }>;
    resolveFace?: (id: number) => string | null;
}) {
    return (
        <div style={{padding: 12, display: "grid", gap: 12}}>
            {sections.map((section) => (
                <div
                    key={section.key}
                    style={{
                        display: "grid",
                        gap: 10,
                        border: "1px solid var(--border)",
                        borderRadius: 10,
                        padding: 10,
                    }}
                >
                    <div style={{display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline"}}>
                        <div style={{fontWeight: 600}}>{section.title}</div>
                        <div className={styles.bandLabel}>{section.ids.length} {"张"}</div>
                    </div>
                    {section.ids.length > 0 ? (
                        <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                            {section.ids.map((id, index) => (
                                <div
                                    key={`${section.key}-${id}-${index}`}
                                    style={{
                                        display: "grid",
                                        gap: 4,
                                        justifyItems: "center",
                                        minWidth: 52,
                                    }}
                                >
                                    <Tile tile={(resolveFace?.(id) || "-")} width={44} height={58}/>
                                    <div className={styles.bandLabel}>#{index + 1}</div>
                                </div>
                            ))}
                        </div>
                    ) : (
                        <div className={styles.cardBodyMuted} style={{padding: 0}}>-</div>
                    )}
                </div>
            ))}
        </div>
    );
}

function QuadCatalogBody({data, resolveFace}: { data: PlanData | null; resolveFace?: (id: number) => string | null }) {
    const {t} = useTranslation();
    if (!data) {
        return <div className={styles.cardBodyMuted}>{t("advisor.awaiting_backend")}</div>;
    }
    const items = data.quad_catalog || [];
    if (!items.length) {
        return <div className={styles.cardBodyMuted}>{t("advisor.quad_catalog_empty")}</div>;
    }
    return (
        <div style={{padding: 12, display: "grid", gap: 12}}>
            {items.map((item, index) => (
                <div key={`${item.face || "quad"}-${index}`} style={{display: "grid", gap: 8, border: "1px solid var(--border)", borderRadius: 10, padding: 10}}>
                    <div style={{fontWeight: 600}}>{t("advisor.quad_catalog_item", {index: index + 1, face: item.face || "-"})}</div>
                    {item.score ? <div className={styles.bandLabel}>{item.score}</div> : null}
                    <QuadTilePositions positions={normalizeQuadPositions(item)} resolveFace={resolveFace}/>
                </div>
            ))}
        </div>
    );
}

function normalizeQuadPositions(item: {
    tile_positions?: Array<{ tile_id: TileId; source: string; source_index: number }>;
    label?: string;
}) {
    if (item.tile_positions && item.tile_positions.length > 0) {
        return item.tile_positions;
    }
    if (!item.label) return [];

    const segments = item.label.split("/").map((part) => part.trim()).filter(Boolean);
    return segments.map((segment) => {
        const idMatch = segment.match(/id=(\d+)/);
        const numMatches = Array.from(segment.matchAll(/(\d+)/g)).map((match) => Number(match[1]));
        const sourceIndex = numMatches.length > 0 ? numMatches[numMatches.length - 1] : NaN;
        return {
            tile_id: idMatch ? Number(idMatch[1]) : NaN,
            source: mapLegacySource(segment),
            source_index: sourceIndex,
        };
    }).filter((entry) => Number.isFinite(entry.tile_id) && Number.isFinite(entry.source_index));
}

function mapLegacySource(source: string) {
    if (source.includes("hand") || source.includes("手牌")) return "hand";
    if (source.includes("replacement") || source.includes("换牌")) return "replacement";
    if (source.includes("wall") || source.includes("牌山")) return "wall";
    return "unknown";
}

function QuadTilePositions({
                               positions,
                               resolveFace,
                           }: {
    positions: Array<{ tile_id: TileId; source: string; source_index: number }>;
    resolveFace?: (id: number) => string | null;
}) {
    const sourceLabel = (source: string) => {
        if (source === "hand") return "手牌";
        if (source === "replacement") return "换牌堆";
        if (source === "wall") return "牌山";
        return "未知来源";
    };

    if (!positions.length) {
        return <div className={styles.cardBodyMuted} style={{padding: 0}}>-</div>;
    }

    return (
        <div style={{display: "flex", flexWrap: "wrap", gap: 10}}>
            {positions.map((item, index) => (
                <div
                    key={`${item.tile_id}-${index}`}
                    style={{
                        border: "1px solid var(--color-divider)",
                        borderRadius: 12,
                        padding: "10px 12px",
                        display: "grid",
                        gap: 8,
                        justifyItems: "center",
                        minWidth: 96,
                        background: "var(--panel)",
                    }}
                >
                    <Tile tile={(resolveFace?.(item.tile_id) || "-")} width={44} height={58}/>
                    <div className={styles.bandLabel}>{sourceLabel(item.source)}</div>
                    <div className={styles.cardBodyMuted} style={{padding: 0}}>{`#${item.source_index}`}</div>
                </div>
            ))}
        </div>
    );
}

function TileGroup({
                       title,
                       ids,
                       resolveFace,
                   }: {
    title: string;
    ids: TileId[];
    resolveFace?: (id: number) => string | null;
}) {
    const {t} = useTranslation();
    return (
        <div style={{display: "grid", gap: 6}}>
            <div className={styles.label}>{title}</div>
            {ids && ids.length > 0 ? (
                <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                    {ids.map((id, index) => (
                        <div key={`${id}-${index}`} className={styles.actionChip}>
                            <span className={`${styles.tilePill} ${styles.tileReset} ${styles.tileRound}`}>
                                <Tile tile={(resolveFace?.(id) || "-")}/>
                            </span>
                            <span className={styles.chipText}>{t("advisor.id_label", {id})}</span>
                        </div>
                    ))}
                </div>
            ) : (
                <div className={styles.cardBodyMuted} style={{padding: 0}}>-</div>
            )}
        </div>
    );
}

function FinalShapeGroup({quadFaces, tenpaiFaces}: { quadFaces: string[]; tenpaiFaces: string[] }) {
    const quadTiles = quadFaces.flatMap((face) => [face, face, face, face]);
    if (!quadTiles.length && !tenpaiFaces.length) {
        return null;
    }

    return (
        <div style={{display: "grid", gap: 6}}>
            <div className={styles.label}>{"最终牌型"}</div>
            <div
                style={{
                    border: "1px solid var(--color-divider)",
                    borderRadius: 14,
                    padding: "10px 12px",
                    display: "flex",
                    flexWrap: "wrap",
                    alignItems: "center",
                    gap: 12,
                    background: "var(--panel)",
                }}
            >
                {quadTiles.length > 0 ? (
                    <div style={{display: "flex", flexWrap: "wrap", gap: 4, alignItems: "center"}}>
                        {quadTiles.map((face, index) => (
                            <Tile key={`quad-${face}-${index}`} tile={face} width={44} height={58}/>
                        ))}
                    </div>
                ) : null}

                {quadTiles.length > 0 && tenpaiFaces.length > 0 ? (
                    <div
                        style={{
                            width: 1,
                            alignSelf: "stretch",
                            minHeight: 44,
                            background: "var(--color-divider)",
                            opacity: 0.8,
                        }}
                    />
                ) : null}

                {tenpaiFaces.length > 0 ? (
                    <div style={{display: "flex", flexWrap: "wrap", gap: 4, alignItems: "center"}}>
                        {tenpaiFaces.map((face, index) => (
                            <Tile key={`tenpai-${face}-${index}`} tile={face} width={44} height={58}/>
                        ))}
                    </div>
                ) : (
                    <div className={styles.cardBodyMuted} style={{padding: 0}}>-</div>
                )}
            </div>
        </div>
    );
}

function FaceChipList({title, faces}: { title: string; faces: string[] }) {
    return (
        <div style={{display: "grid", gap: 6}}>
            <div className={styles.label}>{title}</div>
            {faces && faces.length > 0 ? (
                <div
                    style={{
                        border: "1px solid var(--color-divider)",
                        borderRadius: 14,
                        padding: "10px 12px",
                        display: "flex",
                        flexWrap: "wrap",
                        gap: 4,
                        alignItems: "center",
                        background: "var(--panel)",
                    }}
                >
                    {faces.map((face, index) => (
                        <Tile key={`${face}-${index}`} tile={face} width={44} height={58}/>
                    ))}
                </div>
            ) : (
                <div className={styles.cardBodyMuted} style={{padding: 0}}>-</div>
            )}
        </div>
    );
}

function normalizeBatchList(raw: unknown): TileId[][] {
    if (!Array.isArray(raw)) return [];
    if (raw.length > 0 && !Array.isArray(raw[0])) {
        const flat = (raw as unknown[]).filter((id): id is TileId => typeof id === "number");
        return flat.length ? [flat] : [];
    }
    return raw.map((item) => Array.isArray(item) ? item.filter((id): id is TileId => typeof id === "number") : []);
}

function zipBatches(outRaw?: unknown, incomingRaw?: unknown, sizesRaw?: unknown) {
    const rawOut = normalizeBatchList(outRaw);
    const rawIncoming = normalizeBatchList(incomingRaw);
    const sizes = Array.isArray(sizesRaw) ? sizesRaw.map((n) => Number(n)).filter((n) => Number.isFinite(n) && n >= 0) : [];
    const out = sizes.length && rawOut.length === 1 ? splitFlatBySizes(rawOut[0], sizes) : rawOut;
    const incoming = sizes.length && rawIncoming.length === 1 ? splitFlatBySizes(rawIncoming[0], sizes) : rawIncoming;
    const maxLen = Math.max(out.length, incoming.length, sizes.length);
    return Array.from({length: maxLen}, (_, index) => ({
        out: out[index] || [],
        in: incoming[index] || [],
        size: sizes[index] ?? Math.max((out[index] || []).length, (incoming[index] || []).length, 0),
    }));
}

function reasonText(t: (key: string) => string, reason?: string) {
    if (!reason) return t("advisor.reason_unknown");
    const key = `advisor.reason_${reason}`;
    const translated = t(key);
    return translated === key ? reason : translated;
}

function splitFlatBySizes(flat: TileId[], sizes: number[]) {
    let cursor = 0;
    return sizes.map((size) => {
        const take = Number.isFinite(size) && size > 0 ? size : 0;
        const batch = flat.slice(cursor, cursor + take);
        cursor += take;
        return batch;
    });
}
