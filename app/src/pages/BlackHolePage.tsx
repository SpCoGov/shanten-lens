import React from "react";
import {useTranslation} from "react-i18next";
import {pushToast} from "../lib/toast";
import {ws} from "../lib/ws";
import Tile from "../components/Tile";
import styles from "../components/AdvisorPanel.module.css";
import type {PlanData, TileId} from "../lib/planTypes";

const LS_AUTO_STOP = "sl-blackhole:auto-stop-first";
const LS_VERBOSE = "sl-blackhole:verbose-progress";
const LS_WALL_LIMIT = "sl-blackhole:wall-limit";

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

function summarizeProgress(progress?: string) {
    if (!progress) return "";
    const lines = progress
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line && !line.includes("剩余换牌") && !line.includes("鍓╀綑鎹㈢墝"));
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
        .filter((line) => line && !line.includes("剩余换牌") && !line.includes("鍓╀綑鎹㈢墝"))
        .join("\n");
}

export default function BlackHolePage({
                                          stage,
                                          data,
                                          resolveFace,
                                          onClear,
                                      }: {
    stage: number;
    data: PlanData | null;
    resolveFace?: (id: number) => string | null;
    onClear?: () => void;
}) {
    const {t} = useTranslation();
    const [autoStopFirst, setAutoStopFirst] = React.useState<boolean>(() => readBool(LS_AUTO_STOP, true));
    const [verboseProgress, setVerboseProgress] = React.useState<boolean>(() => readBool(LS_VERBOSE, false));
    const [wallLimit, setWallLimit] = React.useState<number>(() => readWallLimit());
    const [seenSignatures, setSeenSignatures] = React.useState<string[]>([]);
    const [mainData, setMainData] = React.useState<PlanData | null>(null);
    const [quadCatalogData, setQuadCatalogData] = React.useState<PlanData | null>(null);
    const [quadDrawerOpen, setQuadDrawerOpen] = React.useState(false);

    React.useEffect(() => localStorage.setItem(LS_AUTO_STOP, autoStopFirst ? "1" : "0"), [autoStopFirst]);
    React.useEffect(() => localStorage.setItem(LS_VERBOSE, verboseProgress ? "1" : "0"), [verboseProgress]);
    React.useEffect(() => localStorage.setItem(LS_WALL_LIMIT, String(wallLimit)), [wallLimit]);
    React.useEffect(() => {
        if (!data) return;
        if ((data as any).status === "catalog") {
            setQuadCatalogData(data);
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

    const startSearch = React.useCallback((opts?: { stopAfterFirst?: boolean; resume?: boolean }) => {
        if (!canOperate) {
            pushToast(t("blackhole.need_switch_stage"), "error", 1800);
            return;
        }
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
                    wall_limit: wallLimit,
                },
            },
        } as any);
    }, [autoStopFirst, canOperate, planSignature, seenSignatures, t, wallLimit]);

    const stopSearch = React.useCallback(() => {
        ws.send({type: "souzu_switch_control", data: {action: "stop"}} as any);
    }, []);
    const listQuads = React.useCallback(() => {
        setQuadDrawerOpen(true);
        ws.send({type: "souzu_switch_control", data: {action: "list_quads", options: {wall_limit: wallLimit}}} as any);
    }, [wallLimit]);
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
                    <button className="nav-btn" onClick={() => startSearch({stopAfterFirst: false, resume: true})} disabled={!canResume}>
                        {t("blackhole.continue")}
                    </button>
                    <button className="nav-btn" onClick={() => startSearch({stopAfterFirst: true, resume: true})} disabled={!canResume}>
                        {t("blackhole.next")}
                    </button>
                    <button className="nav-btn" onClick={listQuads}>
                        {t("blackhole.list_quads")}
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
                            min={2}
                            max={36}
                            value={wallLimit}
                            onChange={(e) => setWallLimit(Math.min(36, Math.max(2, Number(e.target.value) || 2)))}
                        />
                    </label>
                </div>
                <div style={{marginTop: 10, color: "var(--muted)", fontSize: 13}}>
                    {canOperate ? t("blackhole.stage_ready") : t("blackhole.stage_not_ready")} {t("blackhole.wall_limit_hint")}
                </div>
            </div>

            <div className={`blackhole-layout ${quadDrawerOpen ? "with-drawer" : ""}`}>
                <div className="blackhole-main">
                    <BlackHoleStrategyCard
                        data={viewData}
                        resolveFace={resolveFace}
                        title={t("blackhole.card_title")}
                    />
                </div>

                {quadDrawerOpen && (
                    <aside className="blackhole-drawer panel">
                        <div className="blackhole-drawer-head">
                            <div className="panel-title" style={{marginBottom: 0}}>
                                {t("advisor.quad_catalog_title")}
                            </div>
                            <button className="nav-btn" onClick={() => setQuadDrawerOpen(false)}>
                                {t("modal.close")}
                            </button>
                        </div>
                        <QuadCatalogBody data={quadCatalogData} resolveFace={resolveFace}/>
                    </aside>
                )}
            </div>
        </div>
    );
}

function BlackHoleStrategyCard({
    title,
    data,
    resolveFace,
}: {
    title: string;
    data: PlanData | null;
    resolveFace?: (id: number) => string | null;
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
                    {!hasPlanPreview ? <div style={{padding: "0 12px 12px"}}><SearchParamsBand data={data}/></div> : null}
                    {hasPlanPreview ? <PlanBody data={data} resolveFace={resolveFace}/> : null}
                </div>
            ) : data.status === "impossible" ? (
                <div style={{padding: 12, display: "grid", gap: 12}}>
                    <SearchParamsBand data={data}/>
                    <div className={styles.bandSingle} style={{padding: 0}}>
                        <div className={styles.bandValue}>{t("advisor.impossible")}</div>
                        <div className={styles.cardBodyMuted} style={{padding: 0}}>{reasonText(t, data.reason)}</div>
                    </div>
                </div>
            ) : data.status === "plan" ? (
                <PlanBody data={data} resolveFace={resolveFace}/>
            ) : (
                <div className={styles.cardBodyMuted}>{t("advisor.awaiting_backend")}</div>
            )}
        </section>
    );
}

function PlanBody({data, resolveFace}: { data: PlanData; resolveFace?: (id: number) => string | null }) {
    const {t} = useTranslation();
    const batches = zipBatches(data.switch_discards, data.switch_in, data.switch_batch_sizes);
    return (
        <div style={{padding: 12, display: "grid", gap: 14}}>
            <SearchParamsBand data={data}/>
            <div className={styles.band}>
                <div className={styles.bandLeft} style={{gridColumn: "1 / -1"}}>
                    <div className={styles.bandActionLabel}>{t("advisor.need_draws_label")}</div>
                    <div className={styles.bandValue}>{String(data.draws_needed ?? "-")}</div>
                </div>
            </div>

            {batches.map((batch, index) => (
                <div key={index} className="blackhole-section">
                    <div className={styles.label}>{t("advisor.switch_batch_title", {index: index + 1})}</div>
                    <div className={styles.bandLabel}>{t("advisor.batch_size", {count: batch.size})}</div>
                    <TileGroup title={t("advisor.switch_batch_out", {index: index + 1})} ids={batch.out} resolveFace={resolveFace}/>
                    <TileGroup title={t("advisor.switch_batch_in", {index: index + 1})} ids={batch.in} resolveFace={resolveFace}/>
                </div>
            ))}

            <FaceChipList title={t("advisor.final_quads")} faces={data.quad_faces || []}/>
            <TileGroup title={t("advisor.wall_draw_sequence")} ids={data.wall_draws || []} resolveFace={resolveFace}/>
            <TileGroup title={t("advisor.post_draw_discards")} ids={data.post_draw_discards || []} resolveFace={resolveFace}/>
            <FaceChipList title={t("advisor.final_waits")} faces={data.waits || []}/>
        </div>
    );
}

function SearchParamsBand({data}: { data: PlanData }) {
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
        {label: "最大换牌次数", value: String(data.max_change_count ?? "-")},
        {label: "换牌限制", value: perChangeLimit},
        {label: "可使用的牌数", value: String(data.considered_tile_count ?? "-")},
    ];

    return (
        <div style={{display: "grid", gap: 8}}>
            <div className={styles.label}>当前参数</div>
            <div style={{display: "grid", gridTemplateColumns: "repeat(3, minmax(0, 1fr))", gap: 10}}>
                {items.map((item) => (
                    <div
                        key={item.label}
                        style={{
                            border: "1px solid var(--color-divider)",
                            borderRadius: 10,
                            padding: "10px 12px",
                            display: "grid",
                            gap: 6,
                            minWidth: 0,
                        }}
                    >
                        <div className={styles.bandLabel}>{item.label}</div>
                        <div style={{fontSize: 20, fontWeight: 800, lineHeight: 1.05, wordBreak: "break-word"}}>
                            {item.value}
                        </div>
                    </div>
                ))}
            </div>
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
                    {item.label ? <div className={styles.cardBodyMuted} style={{padding: 0}}>{item.label}</div> : null}
                    {item.score ? <div className={styles.bandLabel}>{item.score}</div> : null}
                    {item.switch_discards?.length ? (
                        zipBatches(item.switch_discards, item.switch_in, item.switch_batch_sizes).map((batch, batchIndex) => (
                            <div key={batchIndex} style={{display: "grid", gap: 8}}>
                                <TileGroup title={t("advisor.switch_batch_out", {index: batchIndex + 1})} ids={batch.out} resolveFace={resolveFace}/>
                                <TileGroup title={t("advisor.switch_batch_in", {index: batchIndex + 1})} ids={batch.in} resolveFace={resolveFace}/>
                            </div>
                        ))
                    ) : null}
                    {!item.reachable && item.reason ? (
                        <div className={styles.cardBodyMuted} style={{padding: 0}}>{item.reason}</div>
                    ) : null}
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

function FaceChipList({title, faces}: { title: string; faces: string[] }) {
    return (
        <div style={{display: "grid", gap: 6}}>
            <div className={styles.label}>{title}</div>
            {faces && faces.length > 0 ? (
                <div style={{display: "flex", flexWrap: "wrap", gap: 8}}>
                    {faces.map((face, index) => (
                        <div key={`${face}-${index}`} className={styles.actionChip}>
                            <span>{face}</span>
                        </div>
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
