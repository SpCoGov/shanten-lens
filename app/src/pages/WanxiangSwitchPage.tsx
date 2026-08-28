import React from "react";
import {useTranslation} from "react-i18next";
import {pushToast} from "../lib/toast";
import * as backendIpc from "../lib/ipc";
import type {PlanData, TileId} from "../lib/planTypes";
import type {GameStateData} from "../lib/gamestate";
import {buildDebugSnapshotFromState} from "./SouzuSwitchDebugPage";
import {BlackHoleStrategyCard} from "./BlackHolePage";

const LS_VERBOSE = "sl-wanxiang-switch:verbose-progress";
const LS_WALL_LIMIT = "sl-wanxiang-switch:wall-limit";
const WALL_LIMIT_MIN = 2;
const WALL_LIMIT_MAX = 36;

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

function isWanxiangPlanData(data: PlanData | null) {
    if (!data) return false;
    return data.search_algorithm === "wanxiang_four_meld_switch"
        || data.mode === "wanxiang-four-meld-switch"
        || String(data.plan_signature || "").startsWith("wanxiang|")
        || data.reason === "wanxiang-not-in-hand"
        || data.reason === "wanxiang-not-reachable-before-draw"
        || data.reason === "cannot-form-four-melds-with-wanxiang";
}

function summarizeProgress(progress?: string) {
    if (!progress) return "";
    const lines = progress
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line && !line.includes("剩余换牌") && !line.includes("当前阶段"));
    return lines.slice(0, 8).join("\n");
}

export default function WanxiangSwitchPage({
                                               stage,
                                               data,
                                               resolveFace,
                                               handIds,
                                               replacementIds,
                                               currentState,
                                               onClear,
                                           }: {
    stage: number;
    data: PlanData | null;
    resolveFace?: (id: number) => string | null;
    handIds: TileId[];
    replacementIds: TileId[];
    wallIds: TileId[];
    currentState: GameStateData | null;
    onClear?: () => void;
}) {
    const {t} = useTranslation();
    const [verboseProgress, setVerboseProgress] = React.useState<boolean>(() => readBool(LS_VERBOSE, false));
    const [wallLimit, setWallLimit] = React.useState<number>(() => readWallLimit());
    const [wallLimitInput, setWallLimitInput] = React.useState<string>(() => String(readWallLimit()));
    const [seenSignatures, setSeenSignatures] = React.useState<string[]>([]);
    const [mainData, setMainData] = React.useState<PlanData | null>(null);

    React.useEffect(() => localStorage.setItem(LS_VERBOSE, verboseProgress ? "1" : "0"), [verboseProgress]);
    React.useEffect(() => localStorage.setItem(LS_WALL_LIMIT, String(wallLimit)), [wallLimit]);
    React.useEffect(() => {
        if (!data || !isWanxiangPlanData(data)) return;
        setMainData(data);
    }, [data]);
    React.useEffect(() => {
        if (!mainData?.plan_signature) return;
        setSeenSignatures((prev) => prev.includes(mainData.plan_signature!) ? prev : [...prev, mainData.plan_signature!]);
    }, [mainData?.plan_signature]);
    React.useEffect(() => {
        if (stage === 4 || stage === 5) return;
        setSeenSignatures([]);
    }, [stage]);
    React.useEffect(() => {
        if (stage === 5 || stage === 4) return;
        if (mainData?.status === "searching") {
            void backendIpc.runSwitch({action: "stop", notify: false});
        }
    }, [stage, mainData?.status]);

    const planSignature = mainData?.plan_signature || "";
    const canOperate = stage === 5 || stage === 4;
    const hasWanxiang = React.useMemo(
        () => [...handIds, ...replacementIds]
            .some((id) => id === 1000 || resolveFace?.(id) === "bd"),
        [handIds, replacementIds, resolveFace],
    );
    const isSearching = mainData?.status === "searching";
    const canResume = !!planSignature && !isSearching;
    const hasExecutablePlan = mainData?.status === "plan";

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

    const startSearch = React.useCallback((opts?: { resume?: boolean }) => {
        if (!canOperate) {
            pushToast(t("blackhole.need_switch_stage"), "error", 1800);
            return;
        }
        if (!hasWanxiang) {
            pushToast(t("blackhole.wanxiang_need"), "error", 2200);
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
        void backendIpc.runSwitch({
            action: "start",
            options: {
                skip_signatures: skipSignatures,
                wall_limit: nextWallLimit,
                search_algorithm: "wanxiang_four_meld_switch",
            },
        });
    }, [canOperate, hasWanxiang, planSignature, resolveWallLimit, seenSignatures, t]);

    const stopSearch = React.useCallback(() => {
        void backendIpc.runSwitch({action: "stop"});
    }, []);
    const executePlan = React.useCallback(() => {
        if (!canOperate || !hasExecutablePlan) return;
        void backendIpc.runSwitch({action: "execute_plan"});
    }, [canOperate, hasExecutablePlan]);
    const exportCurrentSnapshot = React.useCallback(async () => {
        if (!currentState) {
            pushToast(t("blackhole.export_empty"), "error", 1800);
            return;
        }
        const snapshot = buildDebugSnapshotFromState(currentState);
        const text = JSON.stringify(snapshot);
        try {
            await navigator.clipboard.writeText(text);
            pushToast(t("blackhole.export_success_clipboard"), "success", 1600);
        } catch {
            pushToast(t("blackhole.export_success"), "success", 1400);
        }
    }, [currentState, t]);
    const clearCache = React.useCallback(() => {
        setSeenSignatures([]);
        setMainData(null);
        onClear?.();
    }, [onClear]);

    const viewData = React.useMemo(() => {
        if (!mainData || mainData.status !== "searching" || verboseProgress) return mainData;
        return {
            ...mainData,
            progress: summarizeProgress(mainData.progress),
        };
    }, [mainData, verboseProgress]);

    return (
        <div className="settings-wrap wide-page switch-guide-page">
            <section className="panel switch-guide-hero">
                <div className="switch-guide-heading">
                    <div>
                        <h1>{t("blackhole.wanxiang_title")}</h1>
                        {canOperate ? <p>{t("blackhole.wanxiang_stage_ready")}</p> : null}
                    </div>
                    <span className={`switch-guide-status ${canOperate ? "is-ready" : ""}`}>
                        <span aria-hidden="true"/>{t(canOperate ? "blackhole.status_ready" : "blackhole.status_waiting")}
                    </span>
                </div>

                <div className="switch-guide-command-bar">
                    <div className="switch-guide-actions">
                        <button className="switch-guide-button is-primary" onClick={() => startSearch()} disabled={isSearching}>
                            <span className="ms" aria-hidden="true">search</span>{t("blackhole.start")}
                        </button>
                        <button className="switch-guide-button is-danger" onClick={stopSearch} disabled={!isSearching}>
                            <span className="ms" aria-hidden="true">stop_circle</span>{t("blackhole.stop")}
                        </button>
                        <button className="switch-guide-button" onClick={() => startSearch({resume: true})} disabled={!canResume}>
                            <span className="ms" aria-hidden="true">resume</span>{t("blackhole.continue")}
                        </button>
                        <button className="switch-guide-button is-success" onClick={executePlan} disabled={!canOperate || !hasExecutablePlan || isSearching}>
                            <span className="ms" aria-hidden="true">play_arrow</span>{t("blackhole.execute_plan")}
                        </button>
                    </div>

                    <div className="switch-guide-options">
                        <label className="switch-guide-toggle">
                            <input type="checkbox" checked={verboseProgress} onChange={(e) => setVerboseProgress(e.target.checked)}/>
                            <span className="switch-guide-toggle-track" aria-hidden="true"><span/></span>
                            <span>{t("blackhole.verbose_progress")}</span>
                        </label>
                        <label className="switch-guide-number">
                            <span>{t("blackhole.wall_limit")}</span>
                            <input className="form-input" type="number" value={wallLimitInput} onChange={(e) => setWallLimitInput(e.target.value)}/>
                        </label>
                    </div>
                </div>

                <div className="switch-guide-utility-bar">
                    <span>{t("blackhole.wall_limit_hint")}</span>
                    <div>
                        <button type="button" onClick={exportCurrentSnapshot}><span className="ms" aria-hidden="true">content_copy</span>{t("blackhole.export_current")}</button>
                        <button type="button" onClick={clearCache}><span className="ms" aria-hidden="true">delete_sweep</span>{t("blackhole.clear_cache")}</button>
                    </div>
                </div>
            </section>

            <div className="blackhole-layout">
                <div className="blackhole-main">
                    <BlackHoleStrategyCard
                        data={viewData}
                        resolveFace={resolveFace}
                        title={t("blackhole.wanxiang_card_title")}
                    />
                </div>
            </div>
        </div>
    );
}
