import React from "react";
import {createRoot} from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import "./overlay.css";
import "./lib/i18n";
import {ensureI18nReady} from "./lib/i18n";
import {t} from "i18next";
import {ws} from "./lib/ws";
import type {PlanData} from "./lib/planTypes";
import type {EffectItem, GameStateData} from "./lib/gamestate";
import {toDeckMap} from "./lib/gamestate";
import {
    calculateCurrentPoint,
    computeBaseScore,
    formatFixed2,
    parseFixed2,
    parseTargetPointValue,
    projectFuturePoints,
    resolveAmuletRule,
    type AmuletRuleConfig,
    type ResolvedAmuletRule,
} from "./lib/scoreEngine";
import {formatLevelIdToLabel, LEVEL_TARGETS_BY_ID, ORDERED_LEVEL_TARGETS} from "./pages/ScorePage";
import {
    readHudEnabled,
    readHudShowBlackhole,
    readHudShowScoreProjection,
    readHudShowWanxiang,
    HUD_ENABLED_KEY,
    HUD_SHOW_BLACKHOLE_KEY,
    HUD_SHOW_SCORE_PROJECTION_KEY,
    HUD_SHOW_WANXIANG_KEY,
} from "./lib/hudSettings";

type ExecutionState = {
    status: "running" | "completed" | "failed";
    batch_count: number;
    batch_index: number;
    reason: string;
    phase: string;
};

const DEFAULT_SEARCH_ALGORITHM = "target_enumeration_search";
const WANXIANG_SEARCH_ALGORITHM = "wanxiang_four_meld_switch";
const BLACKHOLE_PANEL_STORAGE_KEY = "sl-hud-panel:blackhole";
const WANXIANG_PANEL_STORAGE_KEY = "sl-hud-panel:wanxiang";
const SCORE_PROJECTION_PANEL_STORAGE_KEY = "sl-hud-panel:score-projection";
const SCORE_FAN_STORAGE_KEY = "shanten:point-fan:v1";
const SCORE_WIN_COUNT_STORAGE_KEY = "shanten:point-win-count:v1";
const SCORE_RULES_STORAGE_KEY = "shanten:point-rules:v1";

type PanelBounds = {
    x: number;
    y: number;
    width: number;
    height: number;
    collapsed: boolean;
};

function sendSouzuAction(action: string) {
    ws.send({
        type: "souzu_switch_control",
        data: {
            action,
            options: action === "start"
                ? {wall_limit: 36, search_algorithm: DEFAULT_SEARCH_ALGORITHM}
                : undefined,
        },
    } as any);
}

function sendWanxiangAction(action: string) {
    ws.send({
        type: "souzu_switch_control",
        data: {
            action,
            options: action === "start"
                ? {wall_limit: 36, search_algorithm: WANXIANG_SEARCH_ALGORITHM}
                : undefined,
        },
    } as any);
}

function HudOverlay() {
    const [showBlackhole, setShowBlackhole] = React.useState(() => readHudShowBlackhole());
    const [showWanxiang, setShowWanxiang] = React.useState(() => readHudShowWanxiang());
    const [showScoreProjection, setShowScoreProjection] = React.useState(() => readHudShowScoreProjection());
    const [stage, setStage] = React.useState(0);
    const [gameState, setGameState] = React.useState<GameStateData | null>(null);
    const [plan, setPlan] = React.useState<PlanData | null>(null);
    const [wanxiangPlan, setWanxiangPlan] = React.useState<PlanData | null>(null);
    const [execution, setExecution] = React.useState<ExecutionState | null>(null);
    const blackholePanelRef = React.useRef<HTMLDivElement | null>(null);
    const wanxiangPanelRef = React.useRef<HTMLDivElement | null>(null);
    const scoreProjectionPanelRef = React.useRef<HTMLDivElement | null>(null);
    const visiblePanels = React.useCallback(() => [
        showBlackhole ? blackholePanelRef.current : null,
        showWanxiang ? wanxiangPanelRef.current : null,
        showScoreProjection ? scoreProjectionPanelRef.current : null,
    ], [showBlackhole, showScoreProjection, showWanxiang]);

    React.useEffect(() => {
        ws.connect();
        invoke("set_overlay_enabled", {enabled: readHudEnabled()}).catch(() => {
        });
        const off = ws.onPacket((pkt: any) => {
            if (pkt.type === "update_gamestate") {
                setStage(Number(pkt.data?.stage ?? 0));
                setGameState(pkt.data ?? null);
            } else if (pkt.type === "discard_recommendation") {
                const arr = Array.isArray(pkt.data) ? pkt.data : [];
                for (const item of arr) {
                    if (item?.yaku === "souzu_switch" && item.data?.request_source !== "debug") {
                        if (isWanxiangPlanData(item.data ?? null)) {
                            setWanxiangPlan(item.data ?? null);
                        } else {
                            setPlan(item.data ?? null);
                        }
                    }
                }
            } else if (pkt.type === "souzu_switch_execution" && pkt.data) {
                const d = pkt.data;
                if (d.status === "running" || d.status === "completed" || d.status === "failed") {
                    setExecution({
                        status: d.status,
                        batch_count: Math.max(0, Number(d.batch_count || 0)),
                        batch_index: Math.max(0, Number(d.batch_index || 0)),
                        reason: String(d.reason || ""),
                        phase: String(d.phase || ""),
                    });
                }
            }
        });
        return off;
    }, []);

    React.useEffect(() => {
        const onStorage = (event: StorageEvent) => {
            if (event.key === HUD_ENABLED_KEY) {
                invoke("set_overlay_enabled", {enabled: readHudEnabled()}).catch(() => {
                });
            } else if (event.key === HUD_SHOW_BLACKHOLE_KEY) {
                setShowBlackhole(readHudShowBlackhole());
            } else if (event.key === HUD_SHOW_WANXIANG_KEY) {
                setShowWanxiang(readHudShowWanxiang());
            } else if (event.key === HUD_SHOW_SCORE_PROJECTION_KEY) {
                setShowScoreProjection(readHudShowScoreProjection());
            }
        };
        window.addEventListener("storage", onStorage);
        return () => window.removeEventListener("storage", onStorage);
    }, []);

    React.useEffect(() => {
        const interactive = showBlackhole || showWanxiang || showScoreProjection;
        invoke("set_overlay_interactive", {interactive}).catch(() => {
        });
        reportPanelRegions(visiblePanels());
        return () => {
            invoke("set_overlay_interactive", {interactive: false}).catch(() => {
            });
            reportPanelRegions([]);
        };
    }, [showBlackhole, showScoreProjection, showWanxiang, visiblePanels]);

    React.useEffect(() => {
        if (!showBlackhole && !showWanxiang && !showScoreProjection) return;
        const report = () => reportPanelRegions(visiblePanels());
        report();
        window.addEventListener("resize", report);
        const timer = window.setInterval(report, 500);
        return () => {
            window.removeEventListener("resize", report);
            window.clearInterval(timer);
        };
    }, [showBlackhole, showScoreProjection, showWanxiang, visiblePanels]);

    if (!showBlackhole && !showWanxiang && !showScoreProjection) {
        return <div className="hud-root" aria-hidden="true"/>;
    }

    const isSearching = plan?.status === "searching";
    const hasPlan = plan?.status === "plan" && Array.isArray(plan.switch_discards) && plan.switch_discards.length > 0;
    const wanxiangSearching = wanxiangPlan?.status === "searching";
    const hasWanxiangPlan = wanxiangPlan?.status === "plan" && Array.isArray(wanxiangPlan.switch_discards) && wanxiangPlan.switch_discards.length > 0;
    const canOperate = stage === 2;
    const canWanxiangOperate = stage === 4 || stage === 5;
    const hasWanxiang = hasWanxiangInState(gameState);
    const scoreProjection = getScoreProjection(gameState);

    return (
        <div className="hud-root">
            {showBlackhole ? (
                <HudPanel
                    id="blackhole"
                    className="hud-blackhole"
                    storageKey={BLACKHOLE_PANEL_STORAGE_KEY}
                    defaultBounds={defaultBlackholeBounds()}
                    panelRef={blackholePanelRef}
                    onBoundsChange={() => reportPanelRegions(visiblePanels())}
                    ariaLabel={t("overlay.blackhole_panel")}
                    title={t("overlay.blackhole_panel")}
                >
                    <div className="hud-blackhole-actions">
                        <button onClick={() => sendSouzuAction("start")} disabled={!canOperate || isSearching}>
                            {t("blackhole.start")}
                        </button>
                        <button onClick={() => sendSouzuAction("execute_plan")} disabled={!canOperate || !hasPlan || isSearching}>
                            {t("blackhole.execute_plan")}
                        </button>
                        <button onClick={() => sendSouzuAction("execute_full_plan")} disabled={!canOperate || !hasPlan || isSearching}>
                            {t("blackhole.execute_full_plan")}
                        </button>
                    </div>

                    <div className="hud-blackhole-body">
                        <div className="hud-line">
                            <span>{t("overlay.blackhole_progress")}</span>
                            <strong>{progressText(plan, execution)}</strong>
                        </div>
                        <div className="hud-line">
                            <span>{t("overlay.blackhole_draws_needed")}</span>
                            <strong>{plan?.draws_needed ?? "-"}</strong>
                        </div>
                    </div>
                </HudPanel>
            ) : null}

            {showWanxiang ? (
                <HudPanel
                    id="wanxiang"
                    className="hud-blackhole"
                    storageKey={WANXIANG_PANEL_STORAGE_KEY}
                    defaultBounds={defaultWanxiangBounds()}
                    panelRef={wanxiangPanelRef}
                    onBoundsChange={() => reportPanelRegions(visiblePanels())}
                    ariaLabel={t("overlay.wanxiang_panel")}
                    title={t("overlay.wanxiang_panel")}
                >
                    <div className="hud-blackhole-actions">
                        <button onClick={() => sendWanxiangAction("start")} disabled={!canWanxiangOperate || !hasWanxiang || wanxiangSearching}>
                            {t("blackhole.start")}
                        </button>
                        <button onClick={() => sendWanxiangAction("execute_plan")} disabled={!canWanxiangOperate || !hasWanxiangPlan || wanxiangSearching}>
                            {t("blackhole.execute_plan")}
                        </button>
                    </div>

                    <div className="hud-blackhole-body">
                        <div className="hud-line">
                            <span>{t("overlay.blackhole_progress")}</span>
                            <strong>{progressText(wanxiangPlan, execution)}</strong>
                        </div>
                        <div className="hud-line">
                            <span>{t("blackhole.wanxiang_draws_needed_label")}</span>
                            <strong>{wanxiangPlan?.draws_needed ?? "-"}</strong>
                        </div>
                    </div>
                </HudPanel>
            ) : null}

            {showScoreProjection ? (
                <HudPanel
                    id="score-projection"
                    className="hud-score-projection"
                    storageKey={SCORE_PROJECTION_PANEL_STORAGE_KEY}
                    defaultBounds={defaultScoreProjectionBounds()}
                    panelRef={scoreProjectionPanelRef}
                    onBoundsChange={() => reportPanelRegions(visiblePanels())}
                    ariaLabel={t("overlay.score_projection_panel")}
                    title={t("overlay.score_projection_panel")}
                >
                    <div className="hud-blackhole-body">
                        <div className="hud-line">
                            <span>{t("overlay.score_projection_current_level")}</span>
                            <strong>{scoreProjection.currentLevel}</strong>
                        </div>
                        <div className="hud-line">
                            <span>{t("overlay.score_projection_current_point")}</span>
                            <strong>{scoreProjection.currentPoint}</strong>
                        </div>
                        <div className="hud-line">
                            <span>{t("overlay.score_projection_final_level")}</span>
                            <strong>{scoreProjection.finalLevel}</strong>
                        </div>
                    </div>
                </HudPanel>
            ) : null}
        </div>
    );
}

function HudPanel({
                      className,
                      storageKey,
                      defaultBounds,
                      panelRef,
                      onBoundsChange,
                      ariaLabel,
                      title,
                      children,
                  }: {
    id: string;
    className: string;
    storageKey: string;
    defaultBounds: PanelBounds;
    panelRef: React.MutableRefObject<HTMLDivElement | null>;
    onBoundsChange: () => void;
    ariaLabel: string;
    title: string;
    children: React.ReactNode;
}) {
    const [bounds, setBounds] = React.useState<PanelBounds>(() => readPanelBounds(storageKey, defaultBounds));
    const dragRef = React.useRef<{
        mode: "move" | "resize";
        pointerId: number;
        startX: number;
        startY: number;
        startBounds: PanelBounds;
    } | null>(null);

    React.useLayoutEffect(() => {
        onBoundsChange();
    }, [bounds, onBoundsChange]);

    const updateBounds = React.useCallback((next: PanelBounds) => {
        const clamped = clampBounds(next);
        setBounds(clamped);
        writePanelBounds(storageKey, clamped);
    }, [storageKey]);

    const onPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
        const target = event.target as HTMLElement;
        if (target.closest("button")) return;
        const mode = target.closest(".hud-panel-resize") ? "resize" : "move";
        dragRef.current = {
            mode,
            pointerId: event.pointerId,
            startX: event.clientX,
            startY: event.clientY,
            startBounds: bounds,
        };
        event.currentTarget.setPointerCapture(event.pointerId);
        event.preventDefault();
    };

    const onPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
        const drag = dragRef.current;
        if (!drag || drag.pointerId !== event.pointerId) return;
        const dx = event.clientX - drag.startX;
        const dy = event.clientY - drag.startY;
        if (drag.mode === "resize") {
            updateBounds({
                ...drag.startBounds,
                width: drag.startBounds.width + dx,
                height: drag.startBounds.height + dy,
            });
        } else {
            updateBounds({
                ...drag.startBounds,
                x: drag.startBounds.x + dx,
                y: drag.startBounds.y + dy,
            });
        }
    };

    const onPointerUp = (event: React.PointerEvent<HTMLDivElement>) => {
        if (dragRef.current?.pointerId === event.pointerId) {
            dragRef.current = null;
        }
    };

    const toggleCollapsed = () => {
        updateBounds({...bounds, collapsed: !bounds.collapsed});
    };

    return (
        <section
            ref={panelRef}
            className={`hud-panel ${className} ${bounds.collapsed ? "is-collapsed" : ""}`}
            aria-label={ariaLabel}
            style={{
                left: bounds.x,
                top: bounds.y,
                width: bounds.width,
                height: bounds.collapsed ? undefined : bounds.height,
            }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
        >
            <div className="hud-panel-head">
                <span>{title}</span>
                <button
                    type="button"
                    className="hud-panel-collapse"
                    onClick={toggleCollapsed}
                    aria-label={bounds.collapsed ? t("overlay.expand_panel") : t("overlay.collapse_panel")}
                    title={bounds.collapsed ? t("overlay.expand_panel") : t("overlay.collapse_panel")}
                >
                    {bounds.collapsed ? "+" : "-"}
                </button>
            </div>
            {bounds.collapsed ? null : children}
            {bounds.collapsed ? null : <span className="hud-panel-resize" aria-hidden="true"/>}
        </section>
    );
}

function defaultBlackholeBounds(): PanelBounds {
    const width = Math.min(620, Math.max(320, window.innerWidth - 32));
    const height = 188;
    return {
        x: Math.max(16, Math.round((window.innerWidth - width) / 2)),
        y: Math.max(16, window.innerHeight - height - 18),
        width,
        height,
        collapsed: false,
    };
}

function defaultWanxiangBounds(): PanelBounds {
    const width = Math.min(620, Math.max(320, window.innerWidth - 32));
    const height = 188;
    return {
        x: Math.max(16, Math.round((window.innerWidth - width) / 2)),
        y: Math.max(16, window.innerHeight - height * 2 - 30),
        width,
        height,
        collapsed: false,
    };
}

function defaultScoreProjectionBounds(): PanelBounds {
    const width = Math.min(420, Math.max(320, window.innerWidth - 32));
    const height = 138;
    return {
        x: Math.max(16, window.innerWidth - width - 18),
        y: 18,
        width,
        height,
        collapsed: false,
    };
}

function clampBounds(bounds: PanelBounds): PanelBounds {
    const width = Math.min(Math.max(bounds.width, 320), Math.max(320, window.innerWidth - 16));
    const height = Math.min(Math.max(bounds.height, 118), Math.max(118, window.innerHeight - 16));
    return {
        x: Math.min(Math.max(bounds.x, 0), Math.max(0, window.innerWidth - width)),
        y: Math.min(Math.max(bounds.y, 0), Math.max(0, window.innerHeight - height)),
        width,
        height,
        collapsed: bounds.collapsed,
    };
}

function readPanelBounds(key: string, fallback: PanelBounds): PanelBounds {
    try {
        const raw = localStorage.getItem(key);
        if (!raw) return fallback;
        const parsed = JSON.parse(raw) as Partial<PanelBounds>;
        if (
            typeof parsed.x !== "number" ||
            typeof parsed.y !== "number" ||
            typeof parsed.width !== "number" ||
            typeof parsed.height !== "number"
        ) {
            return fallback;
        }
        return clampBounds({
            x: parsed.x,
            y: parsed.y,
            width: parsed.width,
            height: parsed.height,
            collapsed: typeof parsed.collapsed === "boolean" ? parsed.collapsed : fallback.collapsed,
        });
    } catch {
        return fallback;
    }
}

function writePanelBounds(key: string, bounds: PanelBounds) {
    try {
        localStorage.setItem(key, JSON.stringify(bounds));
    } catch {
    }
}

function reportPanelRegions(panels: Array<HTMLElement | null>) {
    const scale = window.devicePixelRatio || 1;
    const regions = panels
        .filter((panel): panel is HTMLElement => Boolean(panel))
        .map((panel) => {
            const rect = panel.getBoundingClientRect();
            return {
                x: Math.round(rect.left * scale),
                y: Math.round(rect.top * scale),
                width: Math.round(rect.width * scale),
                height: Math.round(rect.height * scale),
            };
        });
    invoke("set_overlay_panel_regions", {regions}).catch(() => {
    });
}

function progressText(plan: PlanData | null, execution: ExecutionState | null) {
    if (execution?.status === "running") {
        const total = execution.batch_count || "-";
        const current = execution.batch_index || 0;
        return `${t("overlay.blackhole_executing")} ${current}/${total}`;
    }
    if (execution?.status === "completed") return t("overlay.blackhole_execution_done");
    if (execution?.status === "failed") return execution.reason || t("overlay.blackhole_execution_failed");
    if (plan?.status === "searching") return plan.progress || t("advisor.searching");
    if (plan?.status === "plan") return t("advisor.badge_switch_plan");
    if (plan?.status === "impossible") return t("advisor.impossible");
    return t("advisor.awaiting_backend");
}

function isWanxiangPlanData(data: PlanData | null) {
    if (!data) return false;
    return data.search_algorithm === WANXIANG_SEARCH_ALGORITHM
        || data.mode === "wanxiang-four-meld-switch"
        || String(data.plan_signature || "").startsWith("wanxiang|")
        || data.reason === "wanxiang-not-in-hand"
        || data.reason === "cannot-form-four-melds-with-wanxiang";
}

function hasWanxiangInState(gameState: GameStateData | null) {
    if (!gameState) return false;
    const deckMap = toDeckMap(gameState.deck_map ?? {});
    return (gameState.hand_tiles ?? []).some((tileId) => tileId === 1000 || deckMap.get(tileId) === "bd");
}

function getScoreProjection(gameState: GameStateData | null) {
    const level = Number(gameState?.level ?? 0);
    const currentLevel = level > 0 ? formatLevelIdToLabel(level) : "-";
    const currentPointValue = parseTargetPointValue(gameState?.point ?? "0");
    const currentPoint = currentPointValue == null ? String(gameState?.point ?? "-") : formatFixed2(currentPointValue);
    if (level <= 0 || gameState == null) {
        return {currentLevel, currentPoint, finalLevel: "-"};
    }

    try {
        const futureProjections = getScorePageFutureProjections(gameState, level);
        let finalLevel = currentLevel;
        for (const projection of futureProjections) {
            if (projection.reached !== true) break;
            finalLevel = formatLevelIdToLabel(projection.level);
        }
        return {currentLevel, currentPoint, finalLevel};
    } catch {
        return {currentLevel, currentPoint, finalLevel: "-"};
    }
}

function getScorePageFutureProjections(gameState: GameStateData, level: number) {
    const startIndex = ORDERED_LEVEL_TARGETS.findIndex((item) => item.level === level);
    if (startIndex < 0) return [];

    const deckMap = toDeckMap(gameState.deck_map ?? {});
    const baseScore = computeBaseScore(gameState.hand_tiles ?? [], deckMap, gameState.tile_score_map ?? {});
    const baseFan = readScoreBaseFan();
    const winCount = readScoreWinCount();
    const customRules = readScoreCustomRules();
    const rules = (gameState.effect_list ?? []).map((item) => resolveAmuletRule(item, customRules[getRuleKey(item)]));
    const hasPinzuInHand = (gameState.hand_tiles ?? []).some((tileId) => isPinzuTile(deckMap.get(tileId)));
    const runtime = {hasPinzuInHand, soulTileCount: 14};
    const seededRules = cloneResolvedRules(rules);
    let seededResult = calculateCurrentPoint(baseScore, baseFan, level, seededRules, runtime);
    for (let winIndex = 1; winIndex < winCount; winIndex += 1) {
        seededResult = calculateCurrentPoint(baseScore, baseFan, level, seededRules, runtime);
    }

    return projectFuturePoints(
        level,
        ORDERED_LEVEL_TARGETS.slice(startIndex + 1).map((item) => ({
            level: item.level,
            target: LEVEL_TARGETS_BY_ID[item.level],
        })),
        seededResult,
        seededRules,
        baseScore,
        baseFan,
        winCount,
        runtime,
    );
}

function cloneResolvedRules(rules: ResolvedAmuletRule[]): ResolvedAmuletRule[] {
    return rules.map((rule) => ({
        ...rule,
        dataRawList: [...rule.dataRawList],
    }));
}

function getRuleKey(item: EffectItem) {
    return `${item.uid}:${item.id}`;
}

function isPinzuTile(tile: string | undefined): boolean {
    return /^[0-9]p$/.test(String(tile ?? "").trim());
}

function readScoreBaseFan() {
    try {
        return parseFixed2(localStorage.getItem(SCORE_FAN_STORAGE_KEY) || "1");
    } catch {
        return parseFixed2("1");
    }
}

function readScoreWinCount() {
    const n = Number.parseInt(String(localStorage.getItem(SCORE_WIN_COUNT_STORAGE_KEY) ?? "1").trim(), 10);
    if (!Number.isFinite(n)) return 1;
    return Math.max(1, Math.min(999, Math.trunc(n)));
}

function readScoreCustomRules(): Record<string, Partial<AmuletRuleConfig>> {
    try {
        const raw = localStorage.getItem(SCORE_RULES_STORAGE_KEY);
        if (!raw) return {};
        return JSON.parse(raw) as Record<string, Partial<AmuletRuleConfig>>;
    } catch {
        return {};
    }
}

ensureI18nReady()
    .catch(() => {
    })
    .finally(() => {
        createRoot(document.getElementById("root")!).render(<HudOverlay/>);
    });
