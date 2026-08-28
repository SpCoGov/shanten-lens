import React from "react";
import {createPortal} from "react-dom";
import "./styles/theme.css";
import "./App.css";
import {invoke} from "@tauri-apps/api/core";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import FrontendTestPage from "./pages/FrontendTestPage";
import AutoRunnerPage from "./pages/AutoRunnerPage";
import FusePage from "./pages/FusePage";
import AboutPage from "./pages/AboutPage";
import BlackHolePage from "./pages/BlackHolePage";
import WanxiangSwitchPage from "./pages/WanxiangSwitchPage";
import SouzuSwitchDebugPage from "./pages/SouzuSwitchDebugPage";
import ScorePage from "./pages/ScorePage";
import OverlayPage from "./pages/OverlayPage";
import TodayWinPage from "./pages/TodayWinPage";
import GameStatePage from "./pages/GameStatePage";
import * as backendIpc from "./lib/ipc";
import {type LogLevel, useLogStore} from "./lib/logStore";
import TileGrid from "./components/TileGrid";
import Modal from "./components/Modal";
import Tile from "./components/Tile";
import WallStats from "./components/WallStats";
import ReplacementPanel from "./components/ReplacementPanel";
import ReplacementStats from "./components/ReplacementStats";
import AdvisorPanel from "./components/AdvisorPanel";
import LevelRecordPanel, {useLevelRecordItems} from "./components/LevelRecordPanel";
import AmuletBar from "./components/AmuletBar";
import {setAppLanguage} from "./lib/i18n";
import {
    buildCells,
    CandidateEffectRef,
    type Cell,
    type EffectItem,
    type GameStateData,
    type GoodsItem,
    toDeckMap,
} from "./lib/gamestate";
import {pushToast, useGlobalToast} from "./lib/toast";
import {AutoRunnerStatus, setAutoStatus, useAutoRunner} from "./lib/autoRunnerStore";
import GoodsBar from "./components/GoodsBar";
import CandidateBar from "./components/CandidateBar";
import "./fonts/material-symbols.css";
import {getCurrentWindow} from "@tauri-apps/api/window";
import "./lib/i18n";
import {useTranslation} from "react-i18next";
import {compareNumericStrings, divideNumericStrings, formatLargeNumber, normalizeNumericString} from "./lib/bigNumber";
import {WebviewWindow, getAllWebviewWindows} from '@tauri-apps/api/webviewWindow';
import {t} from "i18next";
import {openMsgBoxWindow} from "./lib/msgbox";
import type {PlanData} from "./lib/planTypes";
import {buildDoraCountByTile} from "./lib/tileHighlights";
import {APP_VERSION} from "./lib/version";
import {
    checkForUpdates,
    ignoreUpdateVersion,
    readUpdatePrefs,
    setUpdateAutoCheck,
    type UpdateInfo,
} from "./lib/updateCheck";
import {openUrl} from "@tauri-apps/plugin-opener";
import {safeListen} from "./lib/tauriRuntime";
import {formatLevelIdToLabel} from "./lib/levelFormat";
import {useContainerWidth} from "react-grid-layout";
import HomeDashboard from "./components/HomeDashboard";
import PacketPipelinePage from "./pages/PacketPipelinePage";

type BackendLogPayload =
    | string
    | string[]
    | {
    kind?: "lines" | "chunk";
    lines?: string[];
    id?: number;
    index?: number;
    total?: number;
    text?: string;
};

const MAX_BACKEND_LOG_CHARS = 12000;
const BACKEND_LOG_TRUNCATED_SUFFIX = "\n... [truncated in diagnostics view]";

function limitBackendLogLine(line: string) {
    if (line.length <= MAX_BACKEND_LOG_CHARS) return line;
    return line.slice(0, MAX_BACKEND_LOG_CHARS) + BACKEND_LOG_TRUNCATED_SUFFIX;
}

type SouzuSwitchExecutionState = {
    status: "running" | "completed" | "failed";
    batch_count: number;
    batch_index: number;
    reason: string;
    reason_key: string;
    reason_values: Record<string, unknown>;
    phase: string;
    phase_key: string;
    execution_kind: string;
    updated_at: number;
};

type AmuletHotkeySettings = {
    enabled: boolean;
    buyPack: string[];
    selectCandidate: string[];
    skipCandidate: string;
    sellRecent: string;
    refreshShop: string;
    sellMode: "last_list" | "last_selected";
};

const AMULET_HOTKEY_STORAGE_KEY = "sl-amulet-hotkeys";
const DEFAULT_AMULET_HOTKEYS: AmuletHotkeySettings = {
    enabled: false,
    buyPack: ["q", "w", "e", "r", "t"],
    selectCandidate: ["1", "2", "3"],
    skipCandidate: "4",
    sellRecent: "`",
    refreshShop: "f",
    sellMode: "last_list",
};

type HomeSideMode = "records" | "advisor";

type CharacterHealthInfo = {
    characterId: number;
    hp: number;
    maxHp: number;
    percent: number;
    color: string;
};

function normalizeHotkeyKey(value: string): string {
    if (value === " ") return " ";
    const v = String(value || "").trim();
    if (!v) return "";
    if (v === "Space") return " ";
    const parts = v.split("+").map((part) => part.trim()).filter(Boolean);
    if (parts.length <= 1) return normalizeSingleHotkeyKey(v);

    const mods = new Set<string>();
    let main = "";
    for (const part of parts) {
        const lower = part.toLowerCase();
        if (lower === "control" || lower === "ctrl") mods.add("ctrl");
        else if (lower === "alt" || lower === "option") mods.add("alt");
        else if (lower === "shift") mods.add("shift");
        else if (lower === "meta" || lower === "cmd" || lower === "command" || lower === "win") mods.add("meta");
        else main = normalizeSingleHotkeyKey(part);
    }
    if (!main) return "";
    return [...["ctrl", "alt", "shift", "meta"].filter((mod) => mods.has(mod)), main].join("+");
}

function normalizeSingleHotkeyKey(value: string): string {
    const v = String(value || "").trim();
    if (!v) return "";
    if (v === " ") return " ";
    const lower = v.toLowerCase();
    if (lower === "space") return " ";
    if (/^[0-9]$/.test(v)) return `digit${v}`;
    if (/^digit[0-9]$/i.test(v)) return lower;
    if (/^numpad[0-9]$/i.test(v)) return lower;
    if (v.length === 1) return lower;
    return lower;
}

function keyboardCodeKey(event: KeyboardEvent): string {
    if (/^Digit[0-9]$/.test(event.code)) return event.code.toLowerCase();
    if (/^Numpad[0-9]$/.test(event.code)) return event.code.toLowerCase();
    if (event.code === "NumpadDecimal") return "numpaddecimal";
    if (event.code === "NumpadAdd") return "numpadadd";
    if (event.code === "NumpadSubtract") return "numpadsubtract";
    if (event.code === "NumpadMultiply") return "numpadmultiply";
    if (event.code === "NumpadDivide") return "numpaddivide";
    if (event.code === "NumpadEnter") return "numpadenter";
    return "";
}

function keyboardEventKey(event: KeyboardEvent): string {
    if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return "";
    let key: string;
    const codeKey = keyboardCodeKey(event);
    if (codeKey) key = codeKey;
    else if (event.key === "Dead" && event.code === "Backquote") key = "`";
    else if (event.code === "Backquote") key = "`";
    else key = normalizeSingleHotkeyKey(event.key);
    if (!key) return "";
    const mods = [
        event.ctrlKey ? "ctrl" : "",
        event.altKey ? "alt" : "",
        event.shiftKey ? "shift" : "",
        event.metaKey ? "meta" : "",
    ].filter(Boolean);
    return [...mods, key].join("+");
}

function isTypingTarget(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    const tag = target.tagName.toLowerCase();
    return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable;
}

function readAmuletHotkeySettings(): AmuletHotkeySettings {
    try {
        const raw = localStorage.getItem(AMULET_HOTKEY_STORAGE_KEY);
        if (!raw) return DEFAULT_AMULET_HOTKEYS;
        const parsed = JSON.parse(raw) as Partial<AmuletHotkeySettings>;
        return {
            enabled: Boolean(parsed.enabled),
            buyPack: Array.isArray(parsed.buyPack) && parsed.buyPack.length === 5
                ? parsed.buyPack.map((k, i) => normalizeHotkeyKey(k) || DEFAULT_AMULET_HOTKEYS.buyPack[i])
                : DEFAULT_AMULET_HOTKEYS.buyPack,
            selectCandidate: Array.isArray(parsed.selectCandidate) && parsed.selectCandidate.length === 3
                ? parsed.selectCandidate.map((k, i) => normalizeHotkeyKey(k) || DEFAULT_AMULET_HOTKEYS.selectCandidate[i])
                : DEFAULT_AMULET_HOTKEYS.selectCandidate,
            skipCandidate: normalizeHotkeyKey(parsed.skipCandidate || "") || DEFAULT_AMULET_HOTKEYS.skipCandidate,
            sellRecent: normalizeHotkeyKey(parsed.sellRecent || "") || DEFAULT_AMULET_HOTKEYS.sellRecent,
            refreshShop: normalizeHotkeyKey(parsed.refreshShop || "") || DEFAULT_AMULET_HOTKEYS.refreshShop,
            sellMode: parsed.sellMode === "last_selected" ? "last_selected" : "last_list",
        };
    } catch {
        return DEFAULT_AMULET_HOTKEYS;
    }
}

function displayHotkey(value: string): string {
    const normalized = normalizeHotkeyKey(value);
    if (!normalized) return "";
    return normalized.split("+").map((part) => {
        if (part === "ctrl") return "Ctrl";
        if (part === "alt") return "Alt";
        if (part === "shift") return "Shift";
        if (part === "meta") return "Meta";
        if (part === " ") return "Space";
        if (/^digit[0-9]$/.test(part)) return part.slice(5);
        if (/^numpad[0-9]$/.test(part)) return `Num ${part.slice(6)}`;
        if (part === "numpaddecimal") return "Num .";
        if (part === "numpadadd") return "Num +";
        if (part === "numpadsubtract") return "Num -";
        if (part === "numpadmultiply") return "Num *";
        if (part === "numpaddivide") return "Num /";
        if (part === "numpadenter") return "Num Enter";
        return part.length === 1 ? part.toUpperCase() : part;
    }).join("+");
}

function collectAmuletHotkeyEntries(settings: AmuletHotkeySettings): Array<{id: string; key: string}> {
    return [
        ...settings.buyPack.map((key, index) => ({id: `buyPack:${index}`, key: normalizeHotkeyKey(key)})),
        ...settings.selectCandidate.map((key, index) => ({id: `selectCandidate:${index}`, key: normalizeHotkeyKey(key)})),
        {id: "skipCandidate", key: normalizeHotkeyKey(settings.skipCandidate)},
        {id: "sellRecent", key: normalizeHotkeyKey(settings.sellRecent)},
        {id: "refreshShop", key: normalizeHotkeyKey(settings.refreshShop)},
    ].filter((entry) => entry.key);
}

const settingsUrl = import.meta.env.DEV
    ? `${location.origin}/settings.html`
    : 'settings.html';

async function openSettingsWindow() {
    try {
        const existing = (await getAllWebviewWindows()).find(w => w.label === 'settings');
        if (existing) {
            await existing.show();
            await existing.setFocus();
            return;
        }
        const win = new WebviewWindow('settings', {
            url: settingsUrl,
            title: t("settings.title"),
            width: 680,
            height: 580,
            minWidth: 680,
            minHeight: 580,
            center: true,
            resizable: true,
            decorations: false
        });
        win.once('tauri://created', () => console.log('[settings] created'));
        win.once('tauri://error', (e) => console.error('[settings] error', e));
    } catch (err) {
        console.error('[settings] failed to open', err);
    }
}

type Route = "home" | "score" | "blackhole" | "wanxiang" | "souzu-debug" | "fuse" | "pipeline" | "today-win" | "gamestate" | "autorun" | "overlay" | "diagnostics" | "frontend-test" | "about";
type TutorialId = "home" | "blackhole";
type TutorialStep = {
    title: string;
    body: string;
    targetSelector?: string;
};

type VersionMismatch = {
    frontendVersion: string;
    backendVersion: string;
};

type UsageNoticeState = {
    checked: boolean;
};

type TsumoLoopStatus = {
    running: boolean;
    lastReason: string;
    winCount: number;
};

function isWanxiangSwitchPlan(data: PlanData | null | undefined) {
    if (!data) return false;
    return data.search_algorithm === "wanxiang_four_meld_switch"
        || data.mode === "wanxiang-four-meld-switch"
        || String(data.plan_signature || "").startsWith("wanxiang|")
        || data.reason === "wanxiang-not-in-hand"
        || data.reason === "wanxiang-not-reachable-before-draw"
        || data.reason === "cannot-form-four-melds-with-wanxiang";
}

type UpdateDialogState = {
    update: UpdateInfo;
    autoCheck: boolean;
};

const BLACKHOLE_TUTORIAL_SEEN_KEY = "sl-tutorial:blackhole:v1";
const HOME_TUTORIAL_SEEN_KEY = "sl-tutorial:home:v1";
const USAGE_NOTICE_ACK_KEY = "sl-ack-usage-notice";

function readTutorialSeen(key: string) {
    try {
        return localStorage.getItem(key) === "1";
    } catch {
        return true;
    }
}

function writeTutorialSeen(key: string) {
    try {
        localStorage.setItem(key, "1");
    } catch {
        // Ignore storage failures; manual replay still works during this session.
    }
}

function isMoreRoute(route: Route) {
    return route === "fuse"
        || route === "pipeline"
        || route === "wanxiang"
        || route === "today-win"
        || route === "gamestate"
        || route === "overlay"
        || route === "souzu-debug"
        || route === "diagnostics"
        || route === "frontend-test"
        || route === "about";
}

const OUTER_PADDING = 16;
const MAIN_GAP = 12;
const TSUMO_LOOP_INTERVAL_STORAGE_KEY = "sl-tsumo-loop-interval-ms";
const DEFAULT_TSUMO_LOOP_INTERVAL_MS = 400;
const MIN_TSUMO_LOOP_INTERVAL_MS = 0;
const MAX_TSUMO_LOOP_INTERVAL_MS = 10_000;
const SHOP_BUFF_EXCHANGE_ID = 8001;
const SHOP_BUFF_UPGRADE_COSTS = [5, 10, 15, 20, 50, 100, 150, 200];

function clampTsumoLoopIntervalMs(value: unknown) {
    const parsed = Number.parseInt(String(value ?? ""), 10);
    if (!Number.isFinite(parsed)) return DEFAULT_TSUMO_LOOP_INTERVAL_MS;
    return Math.max(MIN_TSUMO_LOOP_INTERVAL_MS, Math.min(MAX_TSUMO_LOOP_INTERVAL_MS, parsed));
}

function readTsumoLoopIntervalMs() {
    try {
        return clampTsumoLoopIntervalMs(localStorage.getItem(TSUMO_LOOP_INTERVAL_STORAGE_KEY));
    } catch {
        return DEFAULT_TSUMO_LOOP_INTERVAL_MS;
    }
}

function writeTsumoLoopIntervalMs(value: number) {
    try {
        localStorage.setItem(TSUMO_LOOP_INTERVAL_STORAGE_KEY, String(clampTsumoLoopIntervalMs(value)));
    } catch {
    }
}

function getAppWindowSafe() {
    try {
        return getCurrentWindow();
    } catch {
        return null;
    }
}

function buildPointProgressMeta(pointRaw?: string, targetRaw?: string) {
    const point = normalizeNumericString(pointRaw ?? "0");
    const target = normalizeNumericString(targetRaw ?? "0");
    if (target === "0") return null;

    const reached = compareNumericStrings(point, target) >= 0;
    const ratioText = divideNumericStrings(point, target, {decimals: 4});
    const ratio = Number.parseFloat(ratioText || "0");
    const cappedFill = reached ? 100 : Math.max(0, Math.min(100, ratio * 100));

    let percentLabel: string | null = null;
    if (reached) {
        let percent = Math.round(Math.min(ratio, 9.99) * 100);
        if (percent < 100) percent = 100;
        if (percent > 999) percent = 999;
        percentLabel = `${percent}%`;
    }

    return {
        fillPercent: Math.max(0, Math.min(100, cappedFill)),
        reached,
        percentLabel,
        centerLabel: reached
            ? `${percentLabel} · ${formatLargeNumber(point)} / ${formatLargeNumber(target)}`
            : `${formatLargeNumber(point)} / ${formatLargeNumber(target)}`,
        pointText: formatLargeNumber(point),
        targetText: formatLargeNumber(target),
    };
}

function TutorialOverlay({
                             steps,
                             onClose,
                         }: {
    steps: TutorialStep[];
    onClose: () => void;
}) {
    const {t} = useTranslation();
    const [stepIndex, setStepIndex] = React.useState(0);
    const [targetRect, setTargetRect] = React.useState<DOMRect | null>(null);
    const cloneLayerRef = React.useRef<HTMLDivElement | null>(null);
    const step = steps[stepIndex];
    const isFirst = stepIndex === 0;
    const isLast = stepIndex === steps.length - 1;

    const updateTargetRect = React.useCallback(() => {
        if (!step?.targetSelector) {
            setTargetRect(null);
            return;
        }
        const target = document.querySelector(step.targetSelector);
        setTargetRect(target instanceof HTMLElement ? target.getBoundingClientRect() : null);
    }, [step?.targetSelector]);

    React.useLayoutEffect(() => {
        updateTargetRect();
        const raf = window.requestAnimationFrame(updateTargetRect);
        window.addEventListener("resize", updateTargetRect);
        window.addEventListener("scroll", updateTargetRect, true);
        return () => {
            window.cancelAnimationFrame(raf);
            window.removeEventListener("resize", updateTargetRect);
            window.removeEventListener("scroll", updateTargetRect, true);
        };
    }, [updateTargetRect]);

    React.useLayoutEffect(() => {
        const layer = cloneLayerRef.current;
        if (!layer) return;
        layer.replaceChildren();
        if (!step?.targetSelector || !targetRect) return;

        const target = document.querySelector(step.targetSelector);
        if (!(target instanceof HTMLElement)) return;

        const clone = target.cloneNode(true) as HTMLElement;
        clone.removeAttribute("id");
        clone.setAttribute("aria-hidden", "true");
        const disableCloneInteractivity = (el: HTMLElement) => {
            el.setAttribute("tabindex", "-1");
            if (el instanceof HTMLButtonElement || el instanceof HTMLInputElement || el instanceof HTMLSelectElement || el instanceof HTMLTextAreaElement) {
                el.disabled = true;
            }
            if (el instanceof HTMLAnchorElement) {
                el.removeAttribute("href");
            }
        };
        disableCloneInteractivity(clone);
        clone.querySelectorAll<HTMLElement>("button, input, select, textarea, a, [tabindex]").forEach(disableCloneInteractivity);
        clone.classList.add("tutorial-target-clone");
        clone.style.left = `${targetRect.left}px`;
        clone.style.top = `${targetRect.top}px`;
        clone.style.width = `${targetRect.width}px`;
        clone.style.height = `${targetRect.height}px`;
        layer.appendChild(clone);

        return () => {
            layer.replaceChildren();
        };
    }, [step?.targetSelector, targetRect]);

    React.useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") onClose();
            if (event.key === "ArrowLeft" && !isFirst) setStepIndex((value) => value - 1);
            if (event.key === "ArrowRight") {
                if (isLast) onClose();
                else setStepIndex((value) => value + 1);
            }
        };
        document.addEventListener("keydown", onKeyDown);
        return () => document.removeEventListener("keydown", onKeyDown);
    }, [isFirst, isLast, onClose]);

    return (
        <div className="tutorial-overlay" role="dialog" aria-modal="true" aria-labelledby="tutorial-title">
            <div className="tutorial-backdrop" aria-hidden="true"/>
            <div className="tutorial-clone-layer" ref={cloneLayerRef} aria-hidden="true"/>
            {targetRect ? (
                <div
                    className="tutorial-spotlight"
                    aria-hidden="true"
                    style={{
                        left: targetRect.left - 8,
                        top: targetRect.top - 8,
                        width: targetRect.width + 16,
                        height: targetRect.height + 16,
                    }}
                />
            ) : null}
            <div className="tutorial-card">
                <div className="tutorial-kicker">
                    {t("tutorial.step_count", {current: stepIndex + 1, total: steps.length})}
                </div>
                <h2 id="tutorial-title">{step.title}</h2>
                <p>{step.body}</p>
                <div className="tutorial-progress" aria-hidden="true">
                    {steps.map((_, index) => (
                        <span key={index} className={index === stepIndex ? "active" : ""}/>
                    ))}
                </div>
                <div className="tutorial-actions">
                    <button className="btn ghost" onClick={onClose}>{t("tutorial.skip")}</button>
                    <div className="tutorial-actions-main">
                        <button className="btn ghost" onClick={() => setStepIndex((value) => Math.max(0, value - 1))} disabled={isFirst}>
                            {t("tutorial.prev")}
                        </button>
                        <button className="btn" onClick={() => isLast ? onClose() : setStepIndex((value) => value + 1)}>
                            {isLast ? t("tutorial.done") : t("tutorial.next")}
                        </button>
                    </div>
                </div>
            </div>
        </div>
    );
}

function Topbar({
                    onSecretClick,
                    stage,
                    point,
                    targetPoint,
                    onTutorialClick,
                    updateAvailable,
                }: {
    onSecretClick: () => void;
    stage: number;
    point?: string;
    targetPoint?: string;
    onTutorialClick?: () => void;
    updateAvailable?: boolean;
}) {
    const {t} = useTranslation();
    const progressMeta = (stage === 4 || stage === 5 || stage === 6 || stage === 7) ? buildPointProgressMeta(point, targetPoint) : null;
    const onMin = async () => {
        const appWindow = getAppWindowSafe();
        if (!appWindow) return;
        try {
            await appWindow.minimize();
        } catch (e) {
            console.error("minimize failed", e);
        }
    };
    const onTgl = async () => {
        const appWindow = getAppWindowSafe();
        if (!appWindow) return;
        try {
            await appWindow.toggleMaximize();
        } catch (e) {
            console.error("toggleMaximize failed", e);
        }
    };
    const onClose = async () => {
        const appWindow = getAppWindowSafe();
        try {
            await invoke("shutdown_app");
        } catch (e) {
            console.error("shutdown failed", e);
            try {
                await appWindow?.close();
            } catch (closeError) {
                console.error("close failed", closeError);
            }
        }
    };

    return (
        <header className={`topbar ${progressMeta ? "has-progress" : ""}`}>
            {progressMeta ? (
                <div className={`topbar-progress-band ${progressMeta.reached ? "is-over" : ""}`} data-tauri-drag-region>
                    <div className="topbar-progress-band-track" data-tauri-drag-region>
                        <div className="topbar-progress-band-fill" style={{width: `${progressMeta.fillPercent}%`}}/>
                    </div>
                    <div className="topbar-progress-band-label" data-tauri-drag-region>{progressMeta.centerLabel}</div>
                </div>
            ) : null}
            <div className="topbar-left drag" data-tauri-drag-region>
                {updateAvailable ? (
                    <span className="update-title-badge" title={t("update.title_badge")} data-tauri-drag-region>
                        <span className="ms" aria-hidden="true">system_update_alt</span>
                    </span>
                ) : null}
                <span className="title" onClick={onSecretClick}>{t("app.title")}</span>
                <span className="title-version-mark" aria-hidden="true" data-tauri-drag-region>v3</span>
            </div>

            <div className="win" data-tauri-drag-region="false">
                {onTutorialClick ? (
                    <button className="win-btn tutorial-replay-btn" data-tauri-drag-region="false" title={t("tutorial.replay")} onClick={onTutorialClick}>
                        <span className="ms">school</span>
                    </button>
                ) : null}
                <button className="win-btn" data-tauri-drag-region="false" title={t("window.minimize")} onClick={onMin}>
                    <span className="ms">remove</span>
                </button>
                <button className="win-btn" data-tauri-drag-region="false" title={t("window.maximize")} onClick={onTgl}>
                    <span className="ms">rectangle</span>
                </button>
                <button className="win-btn close" data-tauri-drag-region="false" title={t("window.close")} onClick={onClose}>
                    <span className="ms">close</span>
                </button>
            </div>
        </header>
    );
}

function HomeSideTabs({
    mode,
    onModeChange,
    recordsAvailable,
}: {
    mode: HomeSideMode;
    onModeChange: (mode: HomeSideMode) => void;
    recordsAvailable: boolean;
}) {
    const {t} = useTranslation();
    return (
        <div className="home-side-tabs" role="tablist" aria-label={t("level_records.tabs_label")}>
            <button
                className={`home-side-tab ${mode === "records" ? "is-active" : ""}`}
                onClick={() => onModeChange("records")}
                role="tab"
                aria-selected={mode === "records"}
                title={t("level_records.tab_records")}
                aria-label={t("level_records.tab_records")}
                disabled={!recordsAvailable}
            >
                <span className="ms" aria-hidden="true">leaderboard</span>
            </button>
            <button
                className={`home-side-tab ${mode === "advisor" ? "is-active" : ""}`}
                onClick={() => onModeChange("advisor")}
                role="tab"
                aria-selected={mode === "advisor"}
                title={t("level_records.tab_advisor")}
                aria-label={t("level_records.tab_advisor")}
            >
                <span className="ms" aria-hidden="true">tips_and_updates</span>
            </button>
        </div>
    );
}

function formatMapNodeValue(value: unknown) {
    if (value === null || value === undefined || value === "") return "-";
    if (typeof value === "object") {
        try {
            return JSON.stringify(value);
        } catch {
            return String(value);
        }
    }
    return String(value);
}

function getGameMapNodeLabel(level: unknown, index: number, count: number) {
    const prefix = formatLevelIdToLabel(level);
    if (index === 0) return `${prefix}-START`;
    if (index === count - 1 && count > 1) return `${prefix}-BOSS`;
    return `${prefix}-${index}`;
}

function getGameMapNodePosition(index: number, count: number): React.CSSProperties {
    const safeCount = Math.max(1, count);
    const x = safeCount === 1 ? 50 : 8 + (84 * index) / (safeCount - 1);
    const y = index % 2 === 0 ? 68 : 24;
    return {
        "--x": `${x}%`,
        "--y": `${y}%`,
    } as React.CSSProperties;
}

function parseFiniteNumber(value: unknown): number | null {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
}

function getCharacterHealthInfo(state?: GameStateData | null): CharacterHealthInfo | null {
    if (!state) return null;
    const characterId = parseFiniteNumber(state.character_id);
    const hp = parseFiniteNumber(state.hp);
    const maxHp = parseFiniteNumber(state.max_hp);
    if (characterId == null || characterId <= 0 || hp == null || maxHp == null || maxHp <= 0) return null;
    const safeHp = Math.max(0, hp);
    const percent = Math.max(0, Math.min(100, (safeHp / maxHp) * 100));
    const hue = Math.round((percent / 100) * 120);
    return {
        characterId: Math.trunc(characterId),
        hp: safeHp,
        maxHp,
        percent,
        color: `hsl(${hue} 72% 42%)`,
    };
}

function CharacterHealthPanel({info}: {info: CharacterHealthInfo}) {
    const {t} = useTranslation();
    const hpText = `${Math.trunc(info.hp)} / ${Math.trunc(info.maxHp)}`;
    return (
        <div className="character-health-panel" title={t("character_health.title")}>
            <img
                className="character-health-portrait"
                src={`/assets/character/character_${info.characterId}.png`}
                alt={t("character_health.portrait_alt", {id: info.characterId})}
                draggable={false}
            />
            <div className="character-health-content">
                <div className="character-health-header">
                    <span>{t("character_health.title")}</span>
                    <strong>{hpText}</strong>
                </div>
                <div
                    className="character-health-bar"
                    aria-label={t("character_health.title")}
                    style={{"--character-health-color": info.color} as React.CSSProperties}
                >
                    <i style={{width: `${info.percent}%`}}/>
                </div>
            </div>
        </div>
    );
}

function getGameMapNodeIconSrc(type: unknown, subType: unknown) {
    if (type === null || type === undefined || subType === null || subType === undefined) return null;
    const typeText = String(type).trim();
    const subTypeText = String(subType).trim();
    if (!typeText || !subTypeText) return null;
    return `/assets/node/${encodeURIComponent(typeText)}-${encodeURIComponent(subTypeText)}.png`;
}

function GameMapModal({
                          open,
                          onClose,
                          currentState,
                      }: {
    open: boolean;
    onClose: () => void;
    currentState: GameStateData | null;
}) {
    const {t} = useTranslation();
    const mapNodes = Array.isArray(currentState?.map_nodes) ? currentState.map_nodes : [];
    const currentNode = Number(currentState?.node ?? 0);
    const displayNodes = mapNodes.length > 0 ? mapNodes : [];

    return (
        <Modal
            open={open}
            onClose={onClose}
            title={t("game_map.title")}
            width={780}
            className="game-map-modal"
        >
            {displayNodes.length > 0 ? (
                <div className="game-map-board">
                    <svg className="game-map-route" viewBox="0 0 1000 360" preserveAspectRatio="none" aria-hidden="true">
                        <path d="M70 250 C145 90 230 105 295 185 S420 280 485 165 S620 70 690 160 S820 285 920 105"/>
                    </svg>
                    <div className="game-map-level-pill">
                        <span>{t("game_map.level")}</span>
                        <strong>{formatLevelIdToLabel(currentState?.level)}</strong>
                    </div>
                    <div className="game-map-node-pill">
                        <span>{t("game_map.node")}</span>
                        <strong>{formatMapNodeValue(currentState?.node)} / {displayNodes.length}</strong>
                    </div>
                    {displayNodes.map((mapNode, index) => {
                        const args = Array.isArray(mapNode.args) ? mapNode.args : [];
                        const isCurrent = currentNode === index + 1;
                        const iconSrc = getGameMapNodeIconSrc(mapNode.type, mapNode.subType);
                        const showTypeInfo = !iconSrc;
                        const showNodeDetails = showTypeInfo || args.length > 0;
                        return (
                            <div
                                className={`game-map-point ${isCurrent ? "is-current" : ""}`}
                                style={getGameMapNodePosition(index, displayNodes.length)}
                                key={index}
                            >
                                <div className={`game-map-marker ${iconSrc ? "has-icon" : ""}`}>
                                    {iconSrc ? (
                                        <img
                                            src={iconSrc}
                                            alt=""
                                            onError={(event) => {
                                                event.currentTarget.style.display = "none";
                                                event.currentTarget.nextElementSibling?.removeAttribute("hidden");
                                            }}
                                        />
                                    ) : null}
                                    <span hidden={Boolean(iconSrc)}>{index + 1}</span>
                                </div>
                                <div className="game-map-label">{getGameMapNodeLabel(currentState?.level, index, displayNodes.length)}</div>
                                {showNodeDetails ? (
                                    <div className="game-map-node-card">
                                        {showTypeInfo ? (
                                            <>
                                                <div><b>{t("game_map.type")}</b>: {formatMapNodeValue(mapNode.type)}</div>
                                                <div><b>{t("game_map.sub_type")}</b>: {formatMapNodeValue(mapNode.subType)}</div>
                                            </>
                                        ) : null}
                                        {args.length > 0 ? (
                                            <div className="game-map-node-card-args">
                                                <b>{t("game_map.args")}</b>: {args.map(formatMapNodeValue).join(", ")}
                                            </div>
                                        ) : null}
                                    </div>
                                ) : null}
                            </div>
                        );
                    })}
                </div>
            ) : (
                <div className="empty">{t("game_map.empty")}</div>
            )}
        </Modal>
    );
}

export default function App() {
    const {t} = useTranslation();
    type ThemeMode = "auto" | "dark" | "dark-green" | "dark-purple";
    const {toast, visible: toastVisible} = useGlobalToast();
    const {config: autoConfig, status: autoStatus} = useAutoRunner();
    const [route, setRoute] = React.useState<Route>("home");
    const sidebarRef = React.useRef<HTMLDivElement | null>(null);
    const {width: homeWidth, containerRef: appMainRef, mounted: homeMounted} = useContainerWidth();
    const navRefs = React.useRef<Partial<Record<Route, HTMLButtonElement | null>>>({});
    const moreButtonRef = React.useRef<HTMLButtonElement | null>(null);
    const moreMenuRef = React.useRef<HTMLDivElement | null>(null);
    const themeButtonRef = React.useRef<HTMLButtonElement | null>(null);
    const [moreMenuOpen, setMoreMenuOpen] = React.useState(false);
    const [connected, setConnected] = React.useState(false);
    const [debugEnabled, setDebugEnabled] = React.useState(false);
    const [versionMismatch, setVersionMismatch] = React.useState<VersionMismatch | null>(null);
    const versionMismatchShownRef = React.useRef(false);
    const [usageNotice, setUsageNotice] = React.useState<UsageNoticeState | null>(null);
    const usageNoticeShownOnStartupRef = React.useRef(false);
    const [updateDialog, setUpdateDialog] = React.useState<UpdateDialogState | null>(null);
    const [latestUpdate, setLatestUpdate] = React.useState<UpdateInfo | null>(null);
    const [updateChecking, setUpdateChecking] = React.useState(false);
    const [tsumoLoopStatus, setTsumoLoopStatus] = React.useState<TsumoLoopStatus>({running: false, lastReason: "", winCount: 0});
    const [tsumoLoopIntervalMs, setTsumoLoopIntervalMs] = React.useState(() => readTsumoLoopIntervalMs());
    const [tsumoLoopSettingsOpen, setTsumoLoopSettingsOpen] = React.useState(false);
    const [tsumoLoopIntervalDraft, setTsumoLoopIntervalDraft] = React.useState(() => String(readTsumoLoopIntervalMs()));
    const updateCheckStartedRef = React.useRef(false);
    const [activeTutorial, setActiveTutorial] = React.useState<TutorialId | null>(null);
    const [souzuSwitchExecution, setSouzuSwitchExecution] = React.useState<SouzuSwitchExecutionState | null>(null);

    const [cells, setCells] = React.useState<Cell[]>([]);
    const [stage, setStage] = React.useState<number>(0);
    const [coin, setCoin] = React.useState<string>("0");
    const [point, setPoint] = React.useState<string>("0");
    const [targetPoint, setTargetPoint] = React.useState<string>("0");
    const [level, setLevel] = React.useState<number>(0);
    const [remain, setRemain] = React.useState<number>(0);
    const [hasGame, setHasGame] = React.useState<boolean>(false);
    const [bossBuff, setBossBuff] = React.useState<number[]>([]);
    const [shopBuffList, setShopBuffList] = React.useState<Record<number, number>>({});

    const [wallStatsTiles, setWallStatsTiles] = React.useState<string[]>([]);
    const [handTileIds, setHandTileIds] = React.useState<number[]>([]);
    const gameEnded = stage === 100;

    const [replacementTiles, setReplacementTiles] = React.useState<string[]>([]);
    const [replacementTileIds, setReplacementTileIds] = React.useState<number[]>([]);
    const [switchUsedCount, setSwitchUsedCount] = React.useState<number>(0);
    const [rightPanelMode, setRightPanelMode] = React.useState<"replacementStats" | "wall">("replacementStats");
    const [wallTileIds, setWallTileIds] = React.useState<number[]>([]);

    const homeTutorialSteps = React.useMemo<TutorialStep[]>(() => [
        {
            title: t("tutorial.home.step_welcome.title"),
            body: t("tutorial.home.step_welcome.body"),
        },
        {
            title: t("tutorial.home.step_home.title"),
            body: t("tutorial.home.step_home.body"),
            targetSelector: '[data-tutorial="nav-home"]',
        },
        {
            title: t("tutorial.home.step_score.title"),
            body: t("tutorial.home.step_score.body"),
            targetSelector: '[data-tutorial="nav-score"]',
        },
        {
            title: t("tutorial.home.step_blackhole.title"),
            body: t("tutorial.home.step_blackhole.body"),
            targetSelector: '[data-tutorial="nav-blackhole"]',
        },
        {
            title: t("tutorial.home.step_autorun.title"),
            body: t("tutorial.home.step_autorun.body"),
            targetSelector: '[data-tutorial="nav-autorun"]',
        },
        {
            title: t("tutorial.home.step_more.title"),
            body: t("tutorial.home.step_more.body"),
            targetSelector: '[data-tutorial="nav-more"]',
        },
        {
            title: t("tutorial.home.step_hotkeys.title"),
            body: t("tutorial.home.step_hotkeys.body"),
            targetSelector: '[data-tutorial="nav-hotkeys"]',
        },
        {
            title: t("tutorial.home.step_refresh.title"),
            body: t("tutorial.home.step_refresh.body"),
            targetSelector: '[data-tutorial="nav-refresh"]',
        },
    ], [t]);

    const blackHoleTutorialSteps = React.useMemo<TutorialStep[]>(() => [
        {
            title: t("tutorial.blackhole.step_welcome.title"),
            body: t("tutorial.blackhole.step_welcome.body"),
        },
        {
            title: t("tutorial.blackhole.step_start.title"),
            body: t("tutorial.blackhole.step_start.body"),
            targetSelector: '[data-tutorial="blackhole-start"]',
        },
        {
            title: t("tutorial.blackhole.step_results.title"),
            body: t("tutorial.blackhole.step_results.body"),
            targetSelector: '[data-tutorial="blackhole-main"]',
        },
        {
            title: t("tutorial.blackhole.step_execute.title"),
            body: t("tutorial.blackhole.step_execute.body"),
            targetSelector: '[data-tutorial="blackhole-execute"]',
        },
        {
            title: t("tutorial.blackhole.step_execute_full.title"),
            body: t("tutorial.blackhole.step_execute_full.body"),
            targetSelector: '[data-tutorial="blackhole-execute-full"]',
        },
        {
            title: t("tutorial.blackhole.step_restart.title"),
            body: t("tutorial.blackhole.step_restart.body"),
        },
    ], [t]);

    const activeTutorialSteps = activeTutorial === "home" ? homeTutorialSteps : blackHoleTutorialSteps;

    const openCurrentTutorial = React.useCallback(() => {
        if (route === "home" || route === "blackhole") {
            setActiveTutorial(route);
        }
    }, [route]);

    const closeTutorial = React.useCallback(() => {
        if (activeTutorial === "home") {
            writeTutorialSeen(HOME_TUTORIAL_SEEN_KEY);
        }
        if (activeTutorial === "blackhole") {
            writeTutorialSeen(BLACKHOLE_TUTORIAL_SEEN_KEY);
        }
        setActiveTutorial(null);
    }, [activeTutorial]);

    React.useEffect(() => {
        if (activeTutorial) return;
        if (route !== "home") return;
        if (readTutorialSeen(HOME_TUTORIAL_SEEN_KEY)) return;
        setActiveTutorial("home");
    }, [activeTutorial, route]);

    React.useEffect(() => {
        if (activeTutorial) return;
        if (route !== "blackhole") return;
        if (readTutorialSeen(BLACKHOLE_TUTORIAL_SEEN_KEY)) return;
        setActiveTutorial("blackhole");
    }, [activeTutorial, route]);

    const [deckMap, setDeckMap] = React.useState<Map<number, string>>(new Map());

    const [planSuuAnkou, setPlanSuuAnkou] = React.useState<PlanData | null>(null);
    const [planChiitoi, setPlanChiitoi] = React.useState<PlanData | null>(null);
    const [planSouzuSwitch, setPlanSouzuSwitch] = React.useState<PlanData | null>(null);
    const [planWanxiangSwitch, setPlanWanxiangSwitch] = React.useState<PlanData | null>(null);
    const [debugSouzuSwitch, setDebugSouzuSwitch] = React.useState<PlanData | null>(null);
    const [latestGameState, setLatestGameState] = React.useState<GameStateData | null>(null);
    const [amuletHotkeys, setAmuletHotkeys] = React.useState<AmuletHotkeySettings>(() => readAmuletHotkeySettings());
    const [gameMapOpen, setGameMapOpen] = React.useState(false);
    const [hotkeyEditorOpen, setHotkeyEditorOpen] = React.useState(false);
    const [sellConfirmTarget, setSellConfirmTarget] = React.useState<EffectItem | null>(null);
    const [lastSelectedCandidateId, setLastSelectedCandidateId] = React.useState<number | null>(null);

    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);
    const [goods, setGoods] = React.useState<GoodsItem[]>([]);
    const [candidates, setCandidates] = React.useState<CandidateEffectRef[]>([]);
    const [tileScoreMap, setTileScoreMap] = React.useState<Record<string, string>>({});
    const [fanValueMap, setFanValueMap] = React.useState<Record<string, string>>({});
    const [homeSideMode, setHomeSideMode] = React.useState<HomeSideMode>("records");
    const levelRecordItems = useLevelRecordItems({level, tileScoreMap, fanValueMap, amulets});
    const hasLevelRecords = levelRecordItems.length > 0;
    const characterHealthInfo = React.useMemo(
        () => getCharacterHealthInfo(latestGameState),
        [latestGameState],
    );
    const isAdvisorStage = stage === 2 || stage === 3;
    const showHomeSidePanel = isAdvisorStage || hasLevelRecords || characterHealthInfo != null;
    const effectiveHomeSideMode: HomeSideMode = isAdvisorStage && !hasLevelRecords ? "advisor" : homeSideMode;

    const THEME_ORDER: ThemeMode[] = ["auto", "dark", "dark-green"];
    const THEME_KEY = "sl-theme";
    const HIDDEN_THEME_CHANCE = 0.01;
    const hiddenThemeClicksRef = React.useRef(0);
    const hiddenThemeClickTimerRef = React.useRef<number | null>(null);

    function applyTheme(t: ThemeMode) {
        const root = document.documentElement;

        root.removeAttribute("data-theme");

        if (t === "auto") {
            const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
            if (prefersDark) {
                root.setAttribute("data-theme", "dark");
            }
            return;
        }

        if (t === "dark" || t === "dark-green" || t === "dark-purple") {
            root.setAttribute("data-theme", t);
        }
    }

    const [theme, setTheme] = React.useState<ThemeMode>(() => {
        const saved = localStorage.getItem(THEME_KEY) as ThemeMode | null;
        if (saved === "auto" || saved === "dark" || saved === "dark-green" || saved === "dark-purple") {
            return saved;
        }
        if (saved === "dark") return "dark";
        return "auto";
    });

    React.useEffect(() => {
        applyTheme(theme);
        localStorage.setItem(THEME_KEY, theme);
    }, [theme]);

    React.useEffect(() => {
        localStorage.setItem(AMULET_HOTKEY_STORAGE_KEY, JSON.stringify(amuletHotkeys));
    }, [amuletHotkeys]);

    React.useEffect(() => {
        writeTsumoLoopIntervalMs(tsumoLoopIntervalMs);
    }, [tsumoLoopIntervalMs]);

    const activateHiddenTheme = React.useCallback(() => {
        if (theme === "dark-purple") return;
        localStorage.setItem(THEME_KEY, "dark-purple");
        setTheme("dark-purple");
        void openMsgBoxWindow({
            id: `hidden-theme-${Date.now()}`,
            title: "app.hidden_theme.title",
            message: "app.hidden_theme.message",
            okText: "common.ok",
            cancelText: undefined,
        });
    }, [theme]);

    const closeProgram = React.useCallback(async () => {
        try {
            await invoke("shutdown_app");
        } catch {
            window.close();
        }
    }, []);

    const openUsageNotice = React.useCallback(() => {
        setUsageNotice({checked: localStorage.getItem(USAGE_NOTICE_ACK_KEY) === "true"});
    }, []);

    const saveUsageNoticeAcknowledgement = React.useCallback((acknowledged: boolean) => {
        localStorage.setItem(USAGE_NOTICE_ACK_KEY, acknowledged ? "true" : "false");
    }, []);

    const closeUsageNotice = React.useCallback(async () => {
        const acknowledged = localStorage.getItem(USAGE_NOTICE_ACK_KEY) === "true";
        if (!acknowledged) {
            await closeProgram();
            return;
        }
        setUsageNotice(null);
    }, [closeProgram]);

    const submitUsageNotice = React.useCallback(async () => {
        const acknowledged = localStorage.getItem(USAGE_NOTICE_ACK_KEY) === "true";
        if (!acknowledged) {
            await closeProgram();
            return;
        }
        setUsageNotice(null);
    }, [closeProgram]);

    const toggleUsageNoticeAcknowledgement = React.useCallback((checked: boolean) => {
        saveUsageNoticeAcknowledgement(checked);
        setUsageNotice((current) => current ? {...current, checked} : current);
    }, [saveUsageNoticeAcknowledgement]);

    React.useEffect(() => {
        if (localStorage.getItem(USAGE_NOTICE_ACK_KEY) === "true" || usageNoticeShownOnStartupRef.current) return;
        usageNoticeShownOnStartupRef.current = true;
        setUsageNotice({checked: false});
    }, []);

    const openUpdateUrl = React.useCallback(async (url: string) => {
        try {
            await openUrl(url);
        } catch (err) {
            console.error("open update url failed", err);
            pushToast(t("update.open_failed"), "error", 2200);
        }
    }, [t]);

    const runUpdateCheck = React.useCallback(async (manual = false) => {
        if (manual) setUpdateChecking(true);
        try {
            const result = await checkForUpdates({manual});
            console.info("[update-check]", manual ? "manual" : "auto", result);
            if (result.status === "available") {
                const prefs = readUpdatePrefs();
                setLatestUpdate(result.update);
                setUpdateDialog({
                    update: result.update,
                    autoCheck: prefs.autoCheck,
                });
                return;
            }
            if (manual && result.status === "current") {
                pushToast(t("update.no_update", {version: APP_VERSION}), "success", 1800);
            } else if (manual && result.status === "disabled") {
                pushToast(t("update.auto_check_disabled"), "info", 1800);
            }
        } catch (err) {
            console.error("[update-check]", manual ? "manual" : "auto", "failed", err);
            if (manual) pushToast(t("update.check_failed"), "error", 2400);
        } finally {
            if (manual) setUpdateChecking(false);
        }
    }, [t]);

    React.useEffect(() => {
        console.info("[update-check]", "auto", "scheduled");
        const timer = window.setTimeout(() => {
            if (updateCheckStartedRef.current) {
                console.info("[update-check]", "auto", "skipped: already started");
                return;
            }
            updateCheckStartedRef.current = true;
            void runUpdateCheck(false);
        }, 1800);
        return () => window.clearTimeout(timer);
    }, [runUpdateCheck]);

    React.useEffect(() => {
        if (Math.random() >= HIDDEN_THEME_CHANCE) return;
        activateHiddenTheme();
    }, [activateHiddenTheme]);

    React.useEffect(() => {
        return () => {
            if (hiddenThemeClickTimerRef.current != null) {
                window.clearTimeout(hiddenThemeClickTimerRef.current);
            }
        };
    }, []);

    const exchangeShopBuffLevel = React.useMemo(() => {
        const raw = (shopBuffList as Record<string, number>)[String(SHOP_BUFF_EXCHANGE_ID)] ?? shopBuffList[SHOP_BUFF_EXCHANGE_ID] ?? 0;
        const parsed = Number(raw);
        if (!Number.isFinite(parsed)) return 0;
        return Math.max(0, Math.trunc(parsed));
    }, [shopBuffList]);

    const nextExchangeShopBuffCost = React.useMemo(
        () => SHOP_BUFF_UPGRADE_COSTS[exchangeShopBuffLevel] ?? null,
        [exchangeShopBuffLevel],
    );

    const handleUpgradeExchangeShopBuff = React.useCallback(async () => {
        if (stage !== 9) {
            pushToast(t("shop_buff_upgrade.stage_not_allowed"), "info", 1800);
            return;
        }
        if (nextExchangeShopBuffCost == null) {
            pushToast(t("shop_buff_upgrade.maxed", {name: t("shop_buff_upgrade.exchange_name")}), "info", 1800);
            return;
        }
        const currentCoin = Number.parseInt(String(coin ?? "0"), 10);
        const normalizedCoin = Number.isFinite(currentCoin) ? currentCoin : 0;
        if (normalizedCoin < nextExchangeShopBuffCost) {
            pushToast(t("shop_buff_upgrade.insufficient_coin", {
                cost: nextExchangeShopBuffCost,
                coin: normalizedCoin,
            }), "error", 2200);
            return;
        }
        const result = await backendIpc.upgradeShopBuff(SHOP_BUFF_EXCHANGE_ID);
        if (result.ok) {
            pushToast(t("shop_buff_upgrade.success", {name: t("shop_buff_upgrade.exchange_name")}), "success", 1800);
        } else if (result.reason === "insufficient_coin") {
            pushToast(t("shop_buff_upgrade.insufficient_coin", {cost: result.cost ?? 0, coin: result.coin ?? 0}), "error", 2200);
        } else if (result.reason === "maxed") {
            pushToast(t("shop_buff_upgrade.maxed", {name: t("shop_buff_upgrade.exchange_name")}), "info", 1800);
        } else if (result.reason === "addon-not-ready") {
            pushToast(t("shop_buff_upgrade.addon_not_ready"), "error", 2200);
        } else {
            pushToast(t("shop_buff_upgrade.failed", {reason: result.reason || "unknown"}), "error", 2600);
        }
    }, [coin, nextExchangeShopBuffCost, stage, t]);

    const toggleTsumoLoop = React.useCallback(async () => {
        const status = tsumoLoopStatus.running
            ? await backendIpc.stopTsumoLoop()
            : await backendIpc.startTsumoLoop(clampTsumoLoopIntervalMs(tsumoLoopIntervalMs));
        setTsumoLoopStatus({running: status.running, lastReason: status.lastReason ?? "", winCount: status.winCount ?? 0});
    }, [tsumoLoopIntervalMs, tsumoLoopStatus.running]);

    const openTsumoLoopSettings = React.useCallback(() => {
        setTsumoLoopIntervalDraft(String(tsumoLoopIntervalMs));
        setTsumoLoopSettingsOpen(true);
    }, [tsumoLoopIntervalMs]);

    const saveTsumoLoopSettings = React.useCallback(() => {
        setTsumoLoopIntervalMs(clampTsumoLoopIntervalMs(tsumoLoopIntervalDraft));
        setTsumoLoopSettingsOpen(false);
    }, [tsumoLoopIntervalDraft]);

    const handleTsumoLoopIntervalDraftChange = React.useCallback((event: React.ChangeEvent<HTMLInputElement>) => {
        setTsumoLoopIntervalDraft(event.currentTarget.value);
    }, []);

    const onSecretClick = React.useCallback(() => {
        hiddenThemeClicksRef.current += 1;
        if (hiddenThemeClickTimerRef.current != null) {
            window.clearTimeout(hiddenThemeClickTimerRef.current);
        }
        hiddenThemeClickTimerRef.current = window.setTimeout(() => {
            hiddenThemeClicksRef.current = 0;
            hiddenThemeClickTimerRef.current = null;
        }, 1600);

        if (hiddenThemeClicksRef.current < 5) return;

        hiddenThemeClicksRef.current = 0;
        if (hiddenThemeClickTimerRef.current != null) {
            window.clearTimeout(hiddenThemeClickTimerRef.current);
            hiddenThemeClickTimerRef.current = null;
        }
        activateHiddenTheme();
    }, [activateHiddenTheme]);

    const themeIcon = (() => {
        switch (theme) {
            case "auto":
                return "light_mode";
            case "dark":
                return "dark_mode";
            case "dark-green":
                return "forest";
            case "dark-purple":
                return "auto_awesome";
            default:
                return "light_mode";
        }
    })();

    const themeLabel = (() => {
        return t("app.theme." + theme);
    })();

    const nextTheme = React.useCallback((t: ThemeMode): ThemeMode => {
        const i = THEME_ORDER.indexOf(t);
        if (i < 0) return THEME_ORDER[0];
        return THEME_ORDER[(i + 1) % THEME_ORDER.length];
    }, []);

    const tianDoraTiles = React.useMemo(
        () => (latestGameState?.tian_dora_tiles ?? []).map((tile) => String(tile ?? "").trim()).filter(Boolean),
        [latestGameState?.tian_dora_tiles],
    );

    const doraCountByTile = React.useMemo(
        () => buildDoraCountByTile(deckMap, latestGameState?.dora_tiles ?? []),
        [deckMap, latestGameState?.dora_tiles],
    );

    const doraIndicatorTiles = React.useMemo(
        () => (latestGameState?.dora_tiles ?? [])
            .map((id) => deckMap.get(id))
            .filter((tile): tile is string => Boolean(tile)),
        [deckMap, latestGameState?.dora_tiles],
    );

    const toggleTheme = React.useCallback((event?: React.MouseEvent<HTMLButtonElement>) => {
        const source = event?.currentTarget ?? themeButtonRef.current;
        const nextModeValue = nextTheme(theme);
        const root = document.documentElement;

        if (source) {
            const rect = source.getBoundingClientRect();
            root.style.setProperty("--theme-wave-x", `${rect.left + rect.width / 2}px`);
            root.style.setProperty("--theme-wave-y", `${rect.top + rect.height / 2}px`);

            const docWithTransition = document as Document & {
                startViewTransition?: (update: () => void) => { finished: Promise<void> };
            };

            if (docWithTransition.startViewTransition) {
                root.classList.add("theme-transition-active");
                const transition = docWithTransition.startViewTransition(() => {
                    setTheme(nextModeValue);
                });
                transition.finished.finally(() => {
                    root.classList.remove("theme-transition-active");
                });
                return;
            }
        }
        setTheme(nextModeValue);
    }, [nextTheme, theme]);

    React.useEffect(() => {
        const update = (sel: HTMLSelectElement) => {
            const hasValue = sel.value !== "" && sel.value != null;
            sel.classList.toggle("has-value", hasValue);
            sel.classList.toggle("is-empty", !hasValue);
        };

        const handlers = new WeakMap<Element, EventListener>();

        const bindAll = (root: ParentNode) => {
            const sels = Array.from(root.querySelectorAll("select")) as HTMLSelectElement[];
            sels.forEach((sel) => {
                update(sel);
                const old = handlers.get(sel);
                if (old) sel.removeEventListener("change", old);
                const h = () => update(sel);
                sel.addEventListener("change", h);
                handlers.set(sel, h);
            });
        };

        bindAll(document);

        const mo = new MutationObserver((muts) => {
            for (const m of muts) {
                m.addedNodes.forEach((n) => {
                    if (n instanceof HTMLElement) bindAll(n);
                });
            }
        });
        mo.observe(document.body, {childList: true, subtree: true});

        return () => {
            document.querySelectorAll("select").forEach((sel) => {
                const h = handlers.get(sel);
                if (h) sel.removeEventListener("change", h);
            });
            mo.disconnect();
        };
    }, []);

    React.useEffect(() => {
        if (stage === 4 || stage === 5) return;
        if (stage === 6 || stage === 7) {
            setRightPanelMode("wall");
            return;
        }
        setRightPanelMode("replacementStats");
    }, [stage]);

    React.useEffect(() => {
        let cancelled = false;

        const finalizeStartup = async () => {
            try {
                await invoke("update_startup_progress", {
                    phase: "render",
                    label: t("startup.finishing"),
                    detail: t("startup.finishing_detail"),
                    progress: 0.97,
                    etaSeconds: 1,
                    indeterminate: false,
                });
            } catch {
            }

            await new Promise((resolve) => window.setTimeout(resolve, 120));
            if (cancelled) return;

            try {
                await invoke("frontend_ready");
            } catch {
            }
        };

        void finalizeStartup();

        return () => {
            cancelled = true;
        };
    }, []);

    React.useEffect(() => {
        type EventName = "update_config" | "update_gamestate" | "discard_recommendation" | "souzu_switch_execution" | "autorun_status" | "tsumo_loop_status" | "msgbox";
        type AppBackendEvent = {[K in EventName]: {type: K; data: backendIpc.BackendEventMap[K]}}[EventName];
        const handleBackendEvent = (pkt: AppBackendEvent) => {
            if (pkt.type === "update_config") {
                const debug = !!(pkt.data as any)?.general?.debug;
                setDebugEnabled(debug);
            } else if (pkt.type === "update_gamestate") {
                const d = pkt.data as GameStateData;
                setLatestGameState(d);
                const deck = toDeckMap(d.deck_map);
                setDeckMap(deck);
                const list = buildCells(deck, d.locked_tiles ?? [], d.wall_tiles ?? [], 36);
                setCells(list);
                setStage(d.stage ?? 0);
                setCoin(d.coin);
                setPoint(typeof d.point === "string" ? d.point : String(d.point ?? "0"));
                setTargetPoint(typeof d.target_point === "string" ? d.target_point : String(d.target_point ?? "0"));
                setLevel(typeof d.level === "number" ? d.level : Number(d.level ?? 0));
                setRemain(d.desktop_remain ?? 0);
                setHasGame(d.stage !== undefined && d.stage >= 0);
                setBossBuff(Array.isArray((d as any).boss_buff) ? (d as any).boss_buff : []);
                setShopBuffList((d as any).shop_buff_list && typeof (d as any).shop_buff_list === "object"
                    ? (d as any).shop_buff_list
                    : {});

                const repl = Array.isArray(d.replacement_tiles)
                    ? d.replacement_tiles.map((id) => deck.get(id) ?? "5m")
                    : [];
                setHandTileIds(Array.isArray(d.hand_tiles) ? d.hand_tiles : []);
                setReplacementTileIds(Array.isArray(d.replacement_tiles) ? d.replacement_tiles : []);
                const used = Array.isArray((d as any).switch_used_tiles) ? (d as any).switch_used_tiles.length : 0;
                setReplacementTiles(repl);
                setSwitchUsedCount(used);

                const wallList = Array.isArray(d.wall_tiles) ? d.wall_tiles.map((id) => deck.get(id) ?? "5m") : [];
                setWallTileIds(Array.isArray(d.wall_tiles) ? d.wall_tiles : []);
                setWallStatsTiles(wallList);

                if (!(d.stage === 4 || d.stage === 5 || d.stage === 6 || stage === 7)) {
                    setPlanSuuAnkou(null);
                    setPlanChiitoi(null);
                }

                setAmulets(Array.isArray(d.effect_list) ? d.effect_list : []);
                setGoods(d.goods ?? []);
                setCandidates(d.candidate_effect_list ?? []);
                setTileScoreMap(d.tile_score_map ?? {});
                setFanValueMap(d.fan_value_map ?? {});
            } else if (pkt.type === "discard_recommendation" && pkt.data) {
                const arr = (Array.isArray(pkt.data) ? pkt.data : []) as Array<{ yaku: string; data: PlanData }>;
                for (const item of arr) {
                    if (!item || !item.yaku) continue;
                    if (item.yaku === "chiitoi") setPlanChiitoi(item.data ?? null);
                    else if (item.yaku === "suuannkou") setPlanSuuAnkou(item.data ?? null);
                    else if (item.yaku === "souzu_switch") {
                        const source = (item.data as any)?.request_source;
                        if (source === "debug") setDebugSouzuSwitch(item.data ?? null);
                        else if (isWanxiangSwitchPlan(item.data)) setPlanWanxiangSwitch(item.data ?? null);
                        else setPlanSouzuSwitch(item.data ?? null);
                    }
                }
            } else if (pkt.type === "souzu_switch_execution" && pkt.data) {
                const d = pkt.data as {
                    status?: string;
                    batch_count?: number;
                    batch_index?: number;
                    reason?: string;
                    reason_key?: string;
                    reason_values?: Record<string, unknown>;
                    phase?: string;
                    phase_key?: string;
                    execution_kind?: string;
                    updated_at?: number;
                };
                if (d.status === "running") {
                    setSouzuSwitchExecution({
                        status: "running",
                        batch_count: Math.max(0, Number(d.batch_count || 0)),
                        batch_index: Math.max(0, Number(d.batch_index || 0)),
                        reason: "",
                        reason_key: "",
                        reason_values: d.reason_values && typeof d.reason_values === "object" ? d.reason_values : {},
                        phase: String(d.phase || ""),
                        phase_key: String(d.phase_key || ""),
                        execution_kind: String(d.execution_kind || "switch"),
                        updated_at: Number(d.updated_at || Date.now()),
                    });
                } else if (d.status === "completed" || d.status === "failed") {
                    setSouzuSwitchExecution({
                        status: d.status,
                        batch_count: Math.max(0, Number(d.batch_count || 0)),
                        batch_index: Math.max(0, Number(d.batch_index || 0)),
                        reason: String(d.reason || ""),
                        reason_key: String(d.reason_key || ""),
                        reason_values: d.reason_values && typeof d.reason_values === "object" ? d.reason_values : {},
                        phase: String(d.phase || ""),
                        phase_key: String(d.phase_key || ""),
                        execution_kind: String(d.execution_kind || "switch"),
                        updated_at: Number(d.updated_at || Date.now()),
                    });
                }
            } else if (pkt.type === "autorun_status" && pkt.data) {
                setAutoStatus(pkt.data as AutoRunnerStatus);
            } else if (pkt.type === "tsumo_loop_status" && pkt.data) {
                const d = pkt.data as Partial<TsumoLoopStatus>;
                setTsumoLoopStatus({
                    running: Boolean(d.running),
                    lastReason: String(d.lastReason || ""),
                    winCount: Math.max(0, Number(d.winCount || 0)),
                });
            } else if (pkt.type === "msgbox" && pkt.data) {
                const d = pkt.data || {};
                if (!d.id) return;
                openMsgBoxWindow({
                    id: String(d.id),
                    title: d.title ? String(d.title) : undefined,
                    message: String(d.message ?? ""),
                    okText: d.okText ? String(d.okText) : undefined,
                    cancelText: d.cancelText ? String(d.cancelText) : undefined,
                    values: d.values ?? undefined,
                });
                return;
            }
        };

        const backendUnlisteners = ([
            "update_config", "update_gamestate", "discard_recommendation", "souzu_switch_execution",
            "autorun_status", "tsumo_loop_status", "msgbox",
        ] as EventName[]).map((name) => backendIpc.subscribeBackendEvent(name, (data) => handleBackendEvent({type: name, data} as AppBackendEvent)));
        void backendIpc.initializeBackend().then((snapshot) => {
            setConnected(true);
            handleBackendEvent({type: "update_config", data: snapshot.config});
            handleBackendEvent({type: "update_gamestate", data: snapshot.gameState});
            handleBackendEvent({type: "autorun_status", data: snapshot.autorunStatus});
            handleBackendEvent({type: "tsumo_loop_status", data: snapshot.tsumoLoopStatus});
            return backendIpc.checkVersion();
        }).then((mismatch) => {
            if (!mismatch || versionMismatchShownRef.current) return;
            versionMismatchShownRef.current = true;
            setVersionMismatch({
                frontendVersion: APP_VERSION,
                backendVersion: String((mismatch as Partial<VersionMismatch>).backendVersion || "unknown"),
            });
        }).catch(() => setConnected(false));

        const addLog = useLogStore.getState().addLog;
        const addLogs = useLogStore.getState().addLogs;
        let unsubs: Array<() => void> = [];
        (async () => {
            const chunkBuffers = new Map<string, {
                total: number;
                parts: string[];
                received: boolean[];
                chars: number;
                truncated: boolean;
            }>();

            const handleBackendLogPayload = (event: string, level: LogLevel, payload: BackendLogPayload) => {
                if (typeof payload === "string") {
                    addLog(level, `${event}: ${limitBackendLogLine(payload)}`);
                    return;
                }

                if (Array.isArray(payload)) {
                    addLogs(level, payload.map((line) => `${event}: ${limitBackendLogLine(line)}`));
                    return;
                }

                if (!payload || typeof payload !== "object") return;

                if (payload.kind === "lines" && Array.isArray(payload.lines)) {
                    addLogs(level, payload.lines.map((line) => `${event}: ${limitBackendLogLine(line)}`));
                    return;
                }

                if (payload.kind === "chunk" && typeof payload.id === "number" && typeof payload.total === "number") {
                    const key = `${event}:${payload.id}`;
                    const bucket = chunkBuffers.get(key) ?? {
                        total: payload.total,
                        parts: Array.from({length: payload.total}, () => ""),
                        received: Array.from({length: payload.total}, () => false),
                        chars: 0,
                        truncated: false,
                    };
                    bucket.total = payload.total;
                    if (typeof payload.index === "number" && payload.index >= 0 && payload.index < bucket.parts.length) {
                        const text = payload.text ?? "";
                        const remaining = Math.max(0, MAX_BACKEND_LOG_CHARS - bucket.chars);
                        bucket.parts[payload.index] = remaining > 0 ? text.slice(0, remaining) : "";
                        bucket.received[payload.index] = true;
                        bucket.chars += bucket.parts[payload.index].length;
                        if (text.length > remaining) bucket.truncated = true;
                    }
                    chunkBuffers.set(key, bucket);
                    if (bucket.received.every(Boolean)) {
                        chunkBuffers.delete(key);
                        addLog(
                            level,
                            `${event}: ${bucket.parts.join("")}${bucket.truncated ? BACKEND_LOG_TRUNCATED_SUFFIX : ""}`,
                        );
                    }
                }
            };

            const sub = async (event: string, level: LogLevel = "INFO") => {
                const un = await safeListen<BackendLogPayload>(event, (e) => {
                    handleBackendLogPayload(event, level, e.payload);
                });
                unsubs.push(un);
            };
            await sub("backend:spawn", "INFO");
            await sub("backend:ready", "INFO");
            await sub("backend:exit", "WARN");
            await sub("backend:error", "ERROR");
        })();

        return () => {
            backendUnlisteners.forEach((unlisten) => unlisten());
            unsubs.forEach((u) => u());
            unsubs = [];
        };
    }, []);

    React.useEffect(() => {
        if (!debugEnabled && route === "souzu-debug") {
            setRoute("diagnostics");
        }
    }, [debugEnabled, route]);

    React.useEffect(() => {
        let un = () => {
        };
        (async () => {
            un = await safeListen<{ lng: string }>("i18n:set-language", (e) => {
                setAppLanguage(e.payload.lng);
            });
        })();
        return () => un();
    }, []);

    const statsHeader = (stage === 4 || stage === 5) ? (
        <div className="right-panel-title-switch" role="tablist" aria-label={t("right_panel.title")}>
            {rightPanelMode === "replacementStats" ? (
                <>
                    <span className="right-panel-title-active">{t("right_panel.replacement")}</span>
                    <button
                        className="right-panel-title-inactive"
                        onClick={() => setRightPanelMode("wall")}
                    >
                        {t("right_panel.wall")}
                    </button>
                </>
            ) : (
                <>
                    <span className="right-panel-title-active">{t("right_panel.wall")}</span>
                    <button
                        className="right-panel-title-inactive"
                        onClick={() => setRightPanelMode("replacementStats")}
                    >
                        {t("right_panel.replacement")}
                    </button>
                </>
            )}
        </div>
    ) : undefined;

    const canUseAmuletHotkeys = React.useMemo(() => [2, 9, 16].includes(stage), [stage]);
    const amuletHotkeyConflicts = React.useMemo(() => {
        const byKey = new Map<string, string[]>();
        for (const entry of collectAmuletHotkeyEntries(amuletHotkeys)) {
            byKey.set(entry.key, [...(byKey.get(entry.key) ?? []), entry.id]);
        }
        const ids = new Set<string>();
        byKey.forEach((entryIds) => {
            if (entryIds.length > 1) {
                entryIds.forEach((id) => ids.add(id));
            }
        });
        return ids;
    }, [amuletHotkeys]);
    const hasAmuletHotkeyConflicts = amuletHotkeyConflicts.size > 0;

    const amuletHotkeyInputClass = React.useCallback((id: string) => (
        amuletHotkeyConflicts.has(id) ? "is-conflict" : ""
    ), [amuletHotkeyConflicts]);

    const setAmuletHotkeyEnabled = React.useCallback((enabled: boolean) => {
        setAmuletHotkeys((current) => ({...current, enabled}));
    }, []);

    const updateAmuletHotkey = React.useCallback((
        group: "buyPack" | "selectCandidate" | "skipCandidate" | "sellRecent" | "refreshShop",
        value: string,
        index?: number,
    ) => {
        const normalized = normalizeHotkeyKey(value);
        if (!normalized) return;
        setAmuletHotkeys((current) => {
            if (group === "buyPack") {
                const buyPack = [...current.buyPack];
                buyPack[index ?? 0] = normalized;
                return {...current, buyPack};
            }
            if (group === "selectCandidate") {
                const selectCandidate = [...current.selectCandidate];
                selectCandidate[index ?? 0] = normalized;
                return {...current, selectCandidate};
            }
            return {...current, [group]: normalized};
        });
    }, []);

    const captureHotkeyInput = React.useCallback((
        event: React.KeyboardEvent<HTMLInputElement>,
        group: "buyPack" | "selectCandidate" | "skipCandidate" | "sellRecent" | "refreshShop",
        index?: number,
    ) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.key === "Tab") return;
        const key = keyboardEventKey(event.nativeEvent);
        if (!key) return;
        updateAmuletHotkey(group, key, index);
    }, [updateAmuletHotkey]);

    const sendAmuletHotkeyAction = React.useCallback(async (request: backendIpc.AmuletActionRequest) => {
        const result = await backendIpc.runAmuletAction(request);
        if (result.ok) {
            const labelKey = request.action === "buy_pack" ? "buy_pack"
                : request.action === "refresh_shop" ? "refresh_shop"
                    : request.action === "sell_recent" || request.action === "sell_effect" ? "sell_recent"
                        : request.action === "sort_effect" ? "sort_effect" : "select_candidate";
            pushToast(t("amulet_hotkeys.toast_sent", {action: t(`amulet_hotkeys.action.${labelKey}`)}), "success", 1200);
            return;
        }
        const reason = result.reason || "unknown";
        const message = reason === "stage-not-allowed" ? t("amulet_hotkeys.stage_unavailable", {stage: result.stage ?? stage})
            : reason === "coin not enough" ? t("amulet_hotkeys.not_enough_coin_short")
                : reason === "no-effects" ? t("amulet_hotkeys.no_effects")
                    : reason === "selected-effect-not-found" ? t("amulet_hotkeys.selected_effect_missing")
                        : reason === "skip-not-allowed" ? t("amulet_hotkeys.free_cannot_skip")
                            : t("amulet_hotkeys.action_failed", {reason});
        pushToast(message, "error", 1800);
    }, [stage, t]);

    const selectCandidateByIndex = React.useCallback((selectedIndex: number) => {
        if (![2, 9, 16].includes(stage)) {
            pushToast(t("amulet_hotkeys.stage_unavailable", {stage}), "info", 1400);
            return;
        }
        const candidate = (candidates ?? [])[selectedIndex];
        setLastSelectedCandidateId(candidate?.id ?? null);
        void sendAmuletHotkeyAction({action: "select_candidate", selectedIndex});
    }, [candidates, sendAmuletHotkeyAction, stage, t]);

    const refreshShopManually = React.useCallback(() => {
        if (stage !== 9) {
            pushToast(t("amulet_hotkeys.stage_unavailable", {stage}), "info", 1400);
            return;
        }
        const currentCoin = Number.parseInt(String(coin ?? "0"), 10);
        const safeCoin = Number.isFinite(currentCoin) ? currentCoin : 0;
        const refreshPrice = Number.parseInt(String(latestGameState?.refresh_price ?? 0), 10);
        if (Number.isFinite(refreshPrice) && safeCoin < refreshPrice) {
            pushToast(t("amulet_hotkeys.not_enough_coin", {cost: refreshPrice, coin: safeCoin}), "error", 1600);
            return;
        }
        void sendAmuletHotkeyAction({action: "refresh_shop"});
    }, [coin, latestGameState?.refresh_price, sendAmuletHotkeyAction, stage, t]);

    const skipCandidateManually = React.useCallback(() => {
        if (![2, 9, 16].includes(stage)) {
            pushToast(t("amulet_hotkeys.stage_unavailable", {stage}), "info", 1400);
            return;
        }
        if (stage === 2) {
            pushToast(t("amulet_hotkeys.free_cannot_skip"), "info", 1400);
            return;
        }
        void sendAmuletHotkeyAction({action: "skip"});
    }, [sendAmuletHotkeyAction, stage, t]);

    const sellOwnedAmulet = React.useCallback((item: EffectItem) => {
        setSellConfirmTarget(null);
        void sendAmuletHotkeyAction({action: "sell_effect", uid: item.uid});
    }, [sendAmuletHotkeyAction]);

    const sortOwnedAmulets = React.useCallback((sortedUid: number[]) => {
        void sendAmuletHotkeyAction({action: "sort_effect", sortedUid});
    }, [sendAmuletHotkeyAction]);

    const handleAmuletHotkey = React.useCallback((event: KeyboardEvent) => {
        if (hotkeyEditorOpen || sellConfirmTarget || usageNotice || versionMismatch || activeTutorial) return;
        if (!amuletHotkeys.enabled || !canUseAmuletHotkeys) return;
        if (isTypingTarget(event.target)) return;

        const key = keyboardEventKey(event);
        if (!key) return;
        const currentCoin = Number.parseInt(String(coin ?? "0"), 10);
        const safeCoin = Number.isFinite(currentCoin) ? currentCoin : 0;

        if (stage === 9) {
            const buyIndex = amuletHotkeys.buyPack.findIndex((item) => normalizeHotkeyKey(item) === key);
            if (buyIndex >= 0) {
                event.preventDefault();
                const good = (goods ?? []).slice(0, 5)[buyIndex];
                if (!good || good.sold) {
                    pushToast(t("amulet_hotkeys.no_pack_slot", {slot: buyIndex + 1}), "info", 1200);
                    return;
                }
                const price = Number.parseInt(String(good.price ?? 0), 10);
                if (Number.isFinite(price) && safeCoin < price) {
                    pushToast(t("amulet_hotkeys.not_enough_coin", {cost: price, coin: safeCoin}), "error", 1600);
                    return;
                }
                void sendAmuletHotkeyAction({action: "buy_pack", goodId: good.id});
                return;
            }

            if (key === normalizeHotkeyKey(amuletHotkeys.refreshShop)) {
                event.preventDefault();
                refreshShopManually();
                return;
            }
        }

        if (stage === 2 || stage === 9 || stage === 16) {
            const candidateIndex = amuletHotkeys.selectCandidate.findIndex((item) => normalizeHotkeyKey(item) === key);
            if (candidateIndex >= 0) {
                event.preventDefault();
                const candidate = (candidates ?? []).slice(0, 3)[candidateIndex];
                if (!candidate) {
                    pushToast(t("amulet_hotkeys.no_amulet_slot", {slot: candidateIndex + 1}), "info", 1200);
                    return;
                }
                selectCandidateByIndex(candidateIndex);
                return;
            }

            if (key === normalizeHotkeyKey(amuletHotkeys.skipCandidate)) {
                event.preventDefault();
                skipCandidateManually();
                return;
            }
        }

        if (key === normalizeHotkeyKey(amuletHotkeys.sellRecent)) {
            event.preventDefault();
            if (amuletHotkeys.sellMode === "last_selected") {
                if (lastSelectedCandidateId == null) {
                    pushToast(t("amulet_hotkeys.no_selected_record"), "info", 1400);
                    return;
                }
                void sendAmuletHotkeyAction({action: "sell_recent", rawId: lastSelectedCandidateId});
                return;
            }
            void sendAmuletHotkeyAction({action: "sell_recent"});
        }
    }, [
        amuletHotkeys,
        canUseAmuletHotkeys,
        candidates,
        coin,
        goods,
        hotkeyEditorOpen,
        lastSelectedCandidateId,
        refreshShopManually,
        sellConfirmTarget,
        selectCandidateByIndex,
        sendAmuletHotkeyAction,
        skipCandidateManually,
        stage,
        t,
        usageNotice,
        versionMismatch,
        activeTutorial,
    ]);

    React.useEffect(() => {
        document.addEventListener("keydown", handleAmuletHotkey);
        return () => document.removeEventListener("keydown", handleAmuletHotkey);
    }, [handleAmuletHotkey]);

    React.useLayoutEffect(() => {
        if (route !== "home") return;
        const main = appMainRef.current;
        if (!main) return;
        main.scrollTop = 0;
        main.scrollLeft = 0;
    }, [route]);

    const navigateFromMore = React.useCallback((nextRoute: Route) => {
        setRoute(nextRoute);
        setMoreMenuOpen(false);
    }, []);

    React.useLayoutEffect(() => {
        const updateSidebarIndicator = () => {
            const sidebar = sidebarRef.current;
            const activeBtn = navRefs.current[route] ?? (isMoreRoute(route) ? moreButtonRef.current : null);
            if (!sidebar || !activeBtn) return;

            let top = activeBtn.offsetTop;
            let offsetParent = activeBtn.offsetParent;
            while (offsetParent instanceof HTMLElement && offsetParent !== sidebar) {
                top += offsetParent.offsetTop;
                offsetParent = offsetParent.offsetParent;
            }
            const height = activeBtn.offsetHeight;

            sidebar.style.setProperty("--nav-indicator-top", `${top}px`);
            sidebar.style.setProperty("--nav-indicator-height", `${height}px`);
        };

        updateSidebarIndicator();

        const ro = new ResizeObserver(() => updateSidebarIndicator());
        if (sidebarRef.current) ro.observe(sidebarRef.current);
        Object.values(navRefs.current).forEach((el) => {
            if (el) ro.observe(el);
        });
        if (moreButtonRef.current) ro.observe(moreButtonRef.current);

        window.addEventListener("resize", updateSidebarIndicator);
        return () => {
            ro.disconnect();
            window.removeEventListener("resize", updateSidebarIndicator);
        };
    }, [route]);

    React.useEffect(() => {
        if (!moreMenuOpen) return;

        const closeOnOutsidePointer = (event: PointerEvent) => {
            const target = event.target as Node | null;
            if (!target) return;
            if (moreButtonRef.current?.contains(target) || moreMenuRef.current?.contains(target)) return;
            setMoreMenuOpen(false);
        };
        const closeOnEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") {
                setMoreMenuOpen(false);
                moreButtonRef.current?.focus();
            }
        };

        document.addEventListener("pointerdown", closeOnOutsidePointer);
        document.addEventListener("keydown", closeOnEscape);
        return () => {
            document.removeEventListener("pointerdown", closeOnOutsidePointer);
            document.removeEventListener("keydown", closeOnEscape);
        };
    }, [moreMenuOpen]);

    return (
        <div className="app">
            <div className="app-ambient" aria-hidden="true">
                <span className="ambient-orb ambient-orb-a"/>
                <span className="ambient-orb ambient-orb-b"/>
                <span className="ambient-grid"/>
            </div>
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>

            <Topbar
                onSecretClick={onSecretClick}
                stage={stage}
                point={point}
                targetPoint={targetPoint}
                onTutorialClick={route === "home" || route === "blackhole" ? openCurrentTutorial : undefined}
                updateAvailable={Boolean(latestUpdate)}
            />

            <div className="shell">
                <aside className="sidebar" ref={sidebarRef}>
                    <div className="sidebar-active-indicator" aria-hidden="true"/>
                    <button ref={(el) => {
                        navRefs.current.home = el;
                    }} className={`nav-icon ${route === "home" ? "active" : ""}`} data-tutorial="nav-home" title={t("nav.home")} onClick={() => setRoute("home")}>
                        <span className="ms">home</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.score = el;
                    }} className={`nav-icon ${route === "score" ? "active" : ""}`} data-tutorial="nav-score" title={t("nav.score")} onClick={() => setRoute("score")}>
                        <span className="ms">calculate</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.blackhole = el;
                    }} className={`nav-icon ${route === "blackhole" ? "active" : ""}`} data-tutorial="nav-blackhole" title={t("nav.blackhole")} onClick={() => setRoute("blackhole")}>
                        <span className="ms">deblur</span>
                    </button>
                    {false && (<button ref={(el) => {
                        navRefs.current.autorun = el;
                    }} className={`nav-icon ${route === "autorun" ? "active" : ""}`} data-tutorial="nav-autorun" title={t("nav.autorun")} onClick={() => setRoute("autorun")}>
                        <span className="ms">autoplay</span>
                    </button>)}
                    <div className="more-nav">
                        <button
                            ref={moreButtonRef}
                            className={`nav-icon ${isMoreRoute(route) ? "active" : ""}`}
                            data-tutorial="nav-more"
                            title={t("nav.more", {defaultValue: t("tutorial.home.step_more.title")})}
                            aria-haspopup="menu"
                            aria-expanded={moreMenuOpen}
                            onClick={() => setMoreMenuOpen((open) => !open)}
                        >
                            <span className="ms">more_horiz</span>
                        </button>
                        {moreMenuOpen && (
                            <div className="more-menu" ref={moreMenuRef} role="menu">
                                <button
                                    className={`more-menu-item ${route === "fuse" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("fuse")}
                                >
                                    <span className="ms">gpp_maybe</span>
                                    <span>{t("nav.fuse")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "pipeline" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("pipeline")}
                                >
                                    <span className="ms">account_tree</span>
                                    <span>{t("nav.pipeline")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "wanxiang" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("wanxiang")}
                                >
                                    <span className="ms">all_inclusive</span>
                                    <span>{t("nav.wanxiang")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "overlay" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("overlay")}
                                >
                                    <span className="ms">picture_in_picture</span>
                                    <span>{t("nav.overlay")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "diagnostics" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("diagnostics")}
                                >
                                    <span className="ms">article</span>
                                    <span>{t("nav.diagnostics")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "about" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("about")}
                                >
                                    <span className="ms">help</span>
                                    <span>{t("nav.about")}</span>
                                </button>

                                <div className="more-menu-divider" role="separator" aria-hidden="true"/>

                                <button
                                    className={`more-menu-item ${route === "today-win" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("today-win")}
                                >
                                    <span className="ms">extension</span>
                                    <span>{t("nav.todayWin")}</span>
                                </button>
                                <button
                                    className={`more-menu-item ${route === "gamestate" ? "active" : ""}`}
                                    role="menuitem"
                                    onClick={() => navigateFromMore("gamestate")}
                                >
                                    <span className="ms">data_object</span>
                                    <span>{t("nav.gamestate")}</span>
                                </button>

                                {debugEnabled && (
                                    <div className="more-menu-divider" role="separator" aria-hidden="true"/>
                                )}

                                {debugEnabled && (
                                    <button
                                        className={`more-menu-item ${route === "frontend-test" ? "active" : ""}`}
                                        role="menuitem"
                                        onClick={() => navigateFromMore("frontend-test")}
                                    >
                                        <span className="ms">lab_profile</span>
                                        <span>{t("nav.frontendTest")}</span>
                                    </button>
                                )}
                                {debugEnabled && (
                                    <button
                                        className={`more-menu-item ${route === "souzu-debug" ? "active" : ""}`}
                                        role="menuitem"
                                        onClick={() => navigateFromMore("souzu-debug")}
                                    >
                                        <span className="ms">science</span>
                                        <span>{t("nav.blackholeDebug")}</span>
                                    </button>
                                )}
                            </div>
                        )}
                    </div>

                    <div className="sidebar-spacer"/>

                    <div className="sidebar-bottom">
                        <button
                            className="nav-icon"
                            data-tutorial="nav-hotkeys"
                            title={t("amulet_hotkeys.customize")}
                            onClick={() => setHotkeyEditorOpen(true)}
                        >
                            <span className="ms">keyboard</span>
                        </button>

                        <button
                            className="nav-icon"
                            data-tutorial="nav-refresh"
                            title={t("nav.refreshGame")}
                            onClick={() => void backendIpc.fetchActivity()}
                        >
                            <span className="ms">refresh</span>
                        </button>

                        <button
                            className="nav-icon"
                            title={t("nav.settings")}
                            onClick={openSettingsWindow}
                        >
                            <span className="ms">settings</span>
                        </button>

                        <button
                            ref={themeButtonRef}
                            className="nav-icon"
                            title={t("app.theme.toggle", {name: themeLabel})}
                            onClick={toggleTheme}
                        >
                            <span className="ms">{themeIcon}</span>
                        </button>
                    </div>
                </aside>

                <main className="main-pane" ref={appMainRef as React.Ref<HTMLElement>}>
                    <div
                        className={`app-main route-${route}`}
                        style={{
                            padding: `${OUTER_PADDING}px ${OUTER_PADDING}px ${route === "home" ? OUTER_PADDING : 0}px ${OUTER_PADDING}px`,
                            boxSizing: "border-box",
                        }}
                    >
                        {route === "home" && (
                            <HomeDashboard tiles={[
                                showHomeSidePanel ? {
                                    id: "side",
                                    content: (
                                    <div className="panel advisor home-advisor-panel home-side-panel">
                                        {isAdvisorStage ? (
                                            <>
                                                <HomeSideTabs
                                                    mode={effectiveHomeSideMode}
                                                    onModeChange={setHomeSideMode}
                                                    recordsAvailable={hasLevelRecords}
                                                />
                                                {effectiveHomeSideMode === "advisor" ? (
                                                    <AdvisorPanel
                                                        suuAnkou={planSuuAnkou}
                                                        chiitoi={planChiitoi}
                                                        resolveFace={(id) => deckMap.get(id) ?? null}
                                                    />
                                                ) : (
                                                    <LevelRecordPanel level={level} items={levelRecordItems}/>
                                                )}
                                                {characterHealthInfo ? (
                                                    <CharacterHealthPanel info={characterHealthInfo}/>
                                                ) : null}
                                            </>
                                        ) : (
                                            <>
                                                <LevelRecordPanel level={level} items={levelRecordItems}/>
                                                {characterHealthInfo ? (
                                                    <CharacterHealthPanel info={characterHealthInfo}/>
                                                ) : null}
                                            </>
                                        )}
                                    </div>
                                    ),
                                } : null,

                                {
                                    id: "main",
                                    splitChildren: true,
                                    content: (
                                <div style={{width: "100%", height: "100%", minWidth: 0, minHeight: 0, overflow: "auto", position: "relative"}}>
                                    <div className="panel" key="amulets">
                                        <div className="panel-title panel-title-with-action">
                                            <span>{t("amulet")}</span>
                                            <div className="panel-title-actions">
                                                <button
                                                    className="panel-title-action"
                                                    onClick={() => setGameMapOpen(true)}
                                                    title={t("game_map.open")}
                                                >
                                                    <span className="ms" aria-hidden="true">map</span>
                                                    <span>{t("game_map.button")}</span>
                                                </button>
                                                <button
                                                    className={`panel-title-action ${tsumoLoopStatus.running ? "active" : ""}`}
                                                    onClick={toggleTsumoLoop}
                                                    title={tsumoLoopStatus.running
                                                        ? t("manual_tsumo.stop_hint")
                                                        : t("manual_tsumo.start_hint")}
                                                >
                                                    <span className="ms" aria-hidden="true">
                                                        {tsumoLoopStatus.running ? "pause" : "play_arrow"}
                                                    </span>
                                                    <span>{tsumoLoopStatus.running ? t("manual_tsumo.stop") : t("manual_tsumo.start")}</span>
                                                    <span className="panel-title-action-count">
                                                        {t("manual_tsumo.win_count", {count: tsumoLoopStatus.winCount})}
                                                    </span>
                                                </button>
                                                <button
                                                    className="panel-title-action"
                                                    onClick={openTsumoLoopSettings}
                                                    title={t("manual_tsumo.settings_hint")}
                                                >
                                                    <span className="ms" aria-hidden="true">settings</span>
                                                    <span>{t("manual_tsumo.interval_value", {value: tsumoLoopIntervalMs})}</span>
                                                </button>
                                            </div>
                                        </div>
                                        <AmuletBar
                                            items={amulets}
                                            scale={0.55}
                                            onItemClick={(item) => setSellConfirmTarget(item)}
                                            onReorder={sortOwnedAmulets}
                                        />
                                    </div>

                                    {(stage === 9) && (
                                        <div className="panel" key="goods">
                                            <div className="panel-title panel-title-with-action">
                                                <span>{t("goods")}</span>
                                                <button
                                                    className="panel-title-action"
                                                    onClick={refreshShopManually}
                                                    disabled={stage !== 9}
                                                    title={t("amulet_hotkeys.refresh_shop_hint", {
                                                        price: latestGameState?.refresh_price ?? 0,
                                                    })}
                                                >
                                                    <span className="ms" aria-hidden="true">refresh</span>
                                                    <span>{t("amulet_hotkeys.refresh_shop_price", {price: latestGameState?.refresh_price ?? 0})}</span>
                                                    {amuletHotkeys.enabled ? (
                                                        <kbd className="panel-title-action-kbd">{displayHotkey(amuletHotkeys.refreshShop)}</kbd>
                                                    ) : null}
                                                </button>
                                            </div>
                                            <GoodsBar
                                                items={goods}
                                                scale={0.85}
                                                hotkeyLabels={amuletHotkeys.enabled ? amuletHotkeys.buyPack.map(displayHotkey) : undefined}
                                            />
                                        </div>
                                    )}

                                    {stage === 9 && false && (
                                        <div className="panel" key="shop-buff">
                                            <div className="panel-title">{t("shop_buff_upgrade.panel_title")}</div>
                                            <div style={{display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center"}}>
                                                <button
                                                    className="nav-btn"
                                                    onClick={handleUpgradeExchangeShopBuff}
                                                    disabled={nextExchangeShopBuffCost == null}
                                                    style={{minWidth: 220}}
                                                    title={nextExchangeShopBuffCost == null
                                                        ? t("shop_buff_upgrade.maxed", {name: t("shop_buff_upgrade.exchange_name")})
                                                        : t("shop_buff_upgrade.button_hint", {
                                                            name: t("shop_buff_upgrade.exchange_name"),
                                                            level: exchangeShopBuffLevel,
                                                            cost: nextExchangeShopBuffCost,
                                                        })}
                                                >
                                                    {nextExchangeShopBuffCost == null
                                                        ? t("shop_buff_upgrade.button_maxed", {
                                                            name: t("shop_buff_upgrade.exchange_name"),
                                                            level: exchangeShopBuffLevel,
                                                        })
                                                        : t("shop_buff_upgrade.button", {
                                                            name: t("shop_buff_upgrade.exchange_name"),
                                                            level: exchangeShopBuffLevel,
                                                            cost: nextExchangeShopBuffCost,
                                                        })}
                                                </button>
                                            </div>
                                        </div>
                                    )}

                                    {[2, 9, 16].includes(stage) && (
                                        <div className="panel" key="candidates">
                                            <div className="panel-title panel-title-with-action">
                                                <span>{t("candidate_amulet")}</span>
                                                {[9, 16].includes(stage) ? (
                                                    <button
                                                        className="panel-title-action"
                                                        onClick={skipCandidateManually}
                                                        title={t("amulet_hotkeys.skip_amulet")}
                                                    >
                                                        <span className="ms" aria-hidden="true">skip_next</span>
                                                        <span>{t("amulet_hotkeys.skip_amulet")}</span>
                                                        {amuletHotkeys.enabled ? (
                                                            <kbd className="panel-title-action-kbd">{displayHotkey(amuletHotkeys.skipCandidate)}</kbd>
                                                        ) : null}
                                                    </button>
                                                ) : null}
                                            </div>
                                            <CandidateBar
                                                candidates={candidates}
                                                ownedAmulets={amulets}
                                                scale={0.55}
                                                max={3}
                                                onCandidateClick={(_candidate, index) => selectCandidateByIndex(index)}
                                                hotkeyLabels={amuletHotkeys.enabled ? amuletHotkeys.selectCandidate.map(displayHotkey) : undefined}
                                            />
                                        </div>
                                    )}

                                    {(stage === 4 || stage === 5 || stage === 6 || stage === 7) && (
                                        <TileGrid
                                            key="tiles"
                                            cells={cells}
                                            tianDoraTiles={tianDoraTiles}
                                            doraCountByTile={doraCountByTile}
                                        />
                                    )}

                                    {(stage === 5 || stage === 4) && replacementTiles.length > 0 && (
                                        <ReplacementPanel key="replacement" replacementTiles={replacementTiles} usedCount={switchUsedCount}/>
                                    )}

                                </div>
                                    ),
                                },

                                (stage === 5 || stage === 4 || stage == 6 || stage === 7) ? {
                                    id: "stats",
                                    splitChildren: true,
                                    content: (
                                    <div className="right-side-panel">
                                        <div className="dora-indicator-panel mj-panel" key="dora">
                                            <div className="dora-indicator-title">{t("dora_indicator.title")}</div>
                                            <div className="dora-indicator-list">
                                                {doraIndicatorTiles.length === 0 ? (
                                                    <div className="dora-indicator-empty">{t("dora_indicator.empty")}</div>
                                                ) : (
                                                    doraIndicatorTiles.map((tile, index) => (
                                                        <div className="dora-indicator-tile" key={`${tile}-${index}`}>
                                                            <Tile
                                                                tile={tile}
                                                                dim={false}
                                                                hoveredTile={null}
                                                                width={30}
                                                                height={40}
                                                            />
                                                        </div>
                                                    ))
                                                )}
                                            </div>
                                        </div>
                                        {(stage === 5 || stage === 4) && rightPanelMode === "replacementStats" ? (
                                            <ReplacementStats key="replacement-stats" replacementTiles={replacementTiles} usedCount={switchUsedCount} headerSlot={statsHeader}/>
                                        ) : (
                                            <WallStats key="wall-stats" wallTiles={wallStatsTiles} headerSlot={statsHeader}/>
                                        )}
                                    </div>
                                    ),
                                } : null,
                            ]} width={homeWidth} mounted={homeMounted}/>
                        )}

                        {route === "score" && (
                            <ScorePage
                                amulets={amulets}
                                handTileIds={handTileIds}
                                deckMap={deckMap}
                                tileScoreMap={tileScoreMap}
                                doraTileIds={latestGameState?.dora_tiles ?? []}
                                tianDoraTiles={latestGameState?.tian_dora_tiles ?? []}
                                level={level}
                                currentPoint={point}
                                currentTargetPoint={targetPoint}
                            />
                        )}
                        {route === "fuse" && <FusePage/>}
                        {route === "pipeline" && <PacketPipelinePage/>}
                        {route === "today-win" && <TodayWinPage/>}
                        {route === "gamestate" && <GameStatePage currentState={latestGameState}/>}
                        {route === "blackhole" && (
                            <BlackHolePage
                                stage={stage}
                                data={planSouzuSwitch}
                                resolveFace={(id) => deckMap.get(id) ?? null}
                                handIds={handTileIds}
                                replacementIds={replacementTileIds}
                                wallIds={wallTileIds}
                                currentState={latestGameState}
                                onClear={() => setPlanSouzuSwitch(null)}
                            />
                        )}
                        {route === "wanxiang" && (
                            <WanxiangSwitchPage
                                stage={stage}
                                data={planWanxiangSwitch}
                                resolveFace={(id) => deckMap.get(id) ?? null}
                                handIds={handTileIds}
                                replacementIds={replacementTileIds}
                                wallIds={wallTileIds}
                                currentState={latestGameState}
                                onClear={() => setPlanWanxiangSwitch(null)}
                            />
                        )}
                        {route === "souzu-debug" && (
                            <SouzuSwitchDebugPage
                                currentState={latestGameState}
                                data={debugSouzuSwitch}
                                onClear={() => setDebugSouzuSwitch(null)}
                            />
                        )}
                        {route === "autorun" && <AutoRunnerPage/>}
                        {route === "overlay" && <OverlayPage/>}
                        {route === "diagnostics" && <DiagnosticsPage/>}
                        {route === "frontend-test" && <FrontendTestPage/>}
                        {route === "about" && (
                            <AboutPage
                                onSecretClick={onSecretClick}
                                onShowUsageNotice={openUsageNotice}
                                onCheckUpdate={() => void runUpdateCheck(true)}
                                updateAvailable={latestUpdate}
                                checkingUpdate={updateChecking}
                            />
                        )}
                    </div>
                </main>
            </div>

            <GameMapModal
                open={gameMapOpen}
                onClose={() => setGameMapOpen(false)}
                currentState={latestGameState}
            />

            <Modal
                open={tsumoLoopSettingsOpen}
                onClose={() => setTsumoLoopSettingsOpen(false)}
                title={t("manual_tsumo.settings_title")}
                width={420}
                actions={(
                    <button className="nav-btn" onClick={saveTsumoLoopSettings}>
                        {t("common.save")}
                    </button>
                )}
            >
                <div className="manual-tsumo-settings">
                    <label className="manual-tsumo-settings-field">
                        <span>{t("manual_tsumo.interval_label")}</span>
                        <div className="manual-tsumo-settings-input-row">
                            <input
                                type="number"
                                min={MIN_TSUMO_LOOP_INTERVAL_MS}
                                max={MAX_TSUMO_LOOP_INTERVAL_MS}
                                step={100}
                                value={tsumoLoopIntervalDraft}
                                onChange={handleTsumoLoopIntervalDraftChange}
                            />
                            <span>{t("manual_tsumo.interval_unit")}</span>
                        </div>
                    </label>
                    <p>{t("manual_tsumo.interval_hint", {
                        min: MIN_TSUMO_LOOP_INTERVAL_MS,
                        max: MAX_TSUMO_LOOP_INTERVAL_MS,
                    })}</p>
                </div>
            </Modal>

            <footer className="statusbar" role="status">
                <div className="sb-left">
          <span className="sb-item">
            <i className={`sb-dot ${connected ? "ok" : "warn"}`}/>
              {connected ? t("status.backendConnected") : t("status.backendDisconnected")}
          </span>
                </div>

                <div className="sb-right">
                    {hasGame ? (
                        <>
                            {autoStatus.running ? (
                                <span className="badge">
                                    {t("status.autorunTargetCount", {
                                        current: autoStatus.current_achieved_count ?? 0,
                                        total: autoConfig.end_count ?? 1,
                                    })}
                                </span>
                            ) : null}
                            <span className="badge">{t("status.remaining", {count: remain})}</span>
                            <span className="badge">{t("status.stage", {stage: stage})}</span>
                            <span className="badge">{t("status.coin", {coin})}</span>
                            {bossBuff.length > 0 ? (
                                <span className="badge">{t("status.bossBuff", {buffs: bossBuff.join(", ")})}</span>
                            ) : null}
                            <span className={`badge ${gameEnded ? "down" : "ok"}`}>{gameEnded ? t("status.ended") : t("status.running")}</span>
                        </>
                    ) : (
                        <span className="badge down">{t("status.noGame")}</span>
                    )}
                </div>
            </footer>
            {activeTutorial === "blackhole" ? (
                <TutorialOverlay steps={activeTutorialSteps} onClose={closeTutorial}/>
            ) : null}
            {activeTutorial === "home" ? (
                <TutorialOverlay steps={activeTutorialSteps} onClose={closeTutorial}/>
            ) : null}
            {hotkeyEditorOpen ? (
                <div
                    className="hotkey-editor-overlay"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="hotkey-editor-title"
                    onClick={() => setHotkeyEditorOpen(false)}
                >
                    <div className="hotkey-editor-panel" onClick={(event) => event.stopPropagation()}>
                        <div className="hotkey-editor-header">
                            <div>
                                <h2 id="hotkey-editor-title">{t("amulet_hotkeys.editor_title")}</h2>
                                <p>{t("amulet_hotkeys.editor_hint")}</p>
                                {hasAmuletHotkeyConflicts ? (
                                    <p className="hotkey-editor-conflict-message">{t("amulet_hotkeys.conflict_hint")}</p>
                                ) : null}
                            </div>
                            <button className="hotkey-editor-close" onClick={() => setHotkeyEditorOpen(false)} aria-label={t("modal.close")}>
                                <span className="ms" aria-hidden="true">close</span>
                            </button>
                        </div>

                        <label
                            className={`hotkey-editor-switch ${amuletHotkeys.enabled ? "is-on" : ""}`}
                            title={amuletHotkeys.enabled ? t("amulet_hotkeys.disable") : t("amulet_hotkeys.enable")}
                        >
                            <input
                                type="checkbox"
                                checked={amuletHotkeys.enabled}
                                onChange={(event) => setAmuletHotkeyEnabled(event.currentTarget.checked)}
                            />
                            <span className="sidebar-hotkey-track" aria-hidden="true">
                                <span className="sidebar-hotkey-thumb"/>
                            </span>
                            <span>{amuletHotkeys.enabled ? t("amulet_hotkeys.disable") : t("amulet_hotkeys.enable")}</span>
                        </label>

                        <div className="hotkey-editor-grid">
                            <div className="hotkey-editor-group">
                                <div className="hotkey-editor-group-title">{t("amulet_hotkeys.group_buy")}</div>
                                {amuletHotkeys.buyPack.map((key, index) => (
                                    <label className={`hotkey-editor-row ${amuletHotkeyInputClass(`buyPack:${index}`)}`} key={`buy-${index}`}>
                                        <span>{t("amulet_hotkeys.pack_slot", {slot: index + 1})}</span>
                                        <input
                                            className={amuletHotkeyInputClass(`buyPack:${index}`)}
                                            value={displayHotkey(key)}
                                            readOnly
                                            onKeyDown={(event) => captureHotkeyInput(event, "buyPack", index)}
                                            onFocus={(event) => event.currentTarget.select()}
                                        />
                                    </label>
                                ))}
                            </div>

                            <div className="hotkey-editor-group">
                                <div className="hotkey-editor-group-title">{t("amulet_hotkeys.group_choose")}</div>
                                {amuletHotkeys.selectCandidate.map((key, index) => (
                                    <label className={`hotkey-editor-row ${amuletHotkeyInputClass(`selectCandidate:${index}`)}`} key={`select-${index}`}>
                                        <span>{t("amulet_hotkeys.amulet_slot", {slot: index + 1})}</span>
                                        <input
                                            className={amuletHotkeyInputClass(`selectCandidate:${index}`)}
                                            value={displayHotkey(key)}
                                            readOnly
                                            onKeyDown={(event) => captureHotkeyInput(event, "selectCandidate", index)}
                                            onFocus={(event) => event.currentTarget.select()}
                                        />
                                    </label>
                                ))}
                                <label className={`hotkey-editor-row ${amuletHotkeyInputClass("skipCandidate")}`}>
                                    <span>{t("amulet_hotkeys.skip_amulet")}</span>
                                    <input
                                        className={amuletHotkeyInputClass("skipCandidate")}
                                        value={displayHotkey(amuletHotkeys.skipCandidate)}
                                        readOnly
                                        onKeyDown={(event) => captureHotkeyInput(event, "skipCandidate")}
                                        onFocus={(event) => event.currentTarget.select()}
                                    />
                                </label>
                            </div>

                            <div className="hotkey-editor-group">
                                <div className="hotkey-editor-group-title">{t("amulet_hotkeys.group_shop")}</div>
                                <label className={`hotkey-editor-row ${amuletHotkeyInputClass("sellRecent")}`}>
                                    <span>{t("amulet_hotkeys.sell_key")}</span>
                                    <input
                                        className={amuletHotkeyInputClass("sellRecent")}
                                        value={displayHotkey(amuletHotkeys.sellRecent)}
                                        readOnly
                                        onKeyDown={(event) => captureHotkeyInput(event, "sellRecent")}
                                        onFocus={(event) => event.currentTarget.select()}
                                    />
                                </label>
                                <div className="hotkey-editor-radio-group" role="radiogroup" aria-label={t("amulet_hotkeys.sell_mode")}>
                                    <label>
                                        <input
                                            type="radio"
                                            checked={amuletHotkeys.sellMode === "last_list"}
                                            onChange={() => setAmuletHotkeys((current) => ({...current, sellMode: "last_list"}))}
                                        />
                                        <span>{t("amulet_hotkeys.sell_mode_last_list")}</span>
                                    </label>
                                    <label>
                                        <input
                                            type="radio"
                                            checked={amuletHotkeys.sellMode === "last_selected"}
                                            onChange={() => setAmuletHotkeys((current) => ({...current, sellMode: "last_selected"}))}
                                        />
                                        <span>{t("amulet_hotkeys.sell_mode_last_selected")}</span>
                                    </label>
                                </div>
                                <label className={`hotkey-editor-row ${amuletHotkeyInputClass("refreshShop")}`}>
                                    <span>{t("amulet_hotkeys.refresh_key")}</span>
                                    <input
                                        className={amuletHotkeyInputClass("refreshShop")}
                                        value={displayHotkey(amuletHotkeys.refreshShop)}
                                        readOnly
                                        onKeyDown={(event) => captureHotkeyInput(event, "refreshShop")}
                                        onFocus={(event) => event.currentTarget.select()}
                                    />
                                </label>
                                <button
                                    className="hotkey-editor-reset"
                                    onClick={() => setAmuletHotkeys({...DEFAULT_AMULET_HOTKEYS, enabled: amuletHotkeys.enabled})}
                                >
                                    {t("amulet_hotkeys.reset_defaults")}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            ) : null}
            {sellConfirmTarget ? (
                <div
                    className="hotkey-editor-overlay"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="sell-amulet-title"
                    onClick={() => setSellConfirmTarget(null)}
                >
                    <div className="sell-amulet-dialog" onClick={(event) => event.stopPropagation()}>
                        <div className="hotkey-editor-header">
                            <div>
                                <h2 id="sell-amulet-title">{t("amulet_hotkeys.sell_confirm_title")}</h2>
                                <p>{t("amulet_hotkeys.sell_confirm_body", {uid: sellConfirmTarget.uid})}</p>
                            </div>
                            <button className="hotkey-editor-close" onClick={() => setSellConfirmTarget(null)} aria-label={t("modal.close")}>
                                <span className="ms" aria-hidden="true">close</span>
                            </button>
                        </div>
                        <div className="sell-amulet-preview">
                            <AmuletBar items={[sellConfirmTarget]} scale={0.62} max={1} showPrice/>
                        </div>
                        <div className="sell-amulet-actions">
                            <button className="nav-btn" onClick={() => setSellConfirmTarget(null)}>
                                {t("common.cancel")}
                            </button>
                            <button className="nav-btn sell-amulet-danger" onClick={() => sellOwnedAmulet(sellConfirmTarget)}>
                                {t("amulet_hotkeys.sell_confirm_action")}
                            </button>
                        </div>
                    </div>
                </div>
            ) : null}
            {usageNotice ? (
                <div
                    className="usage-notice-overlay"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="usage-notice-title"
                    onClick={() => void closeUsageNotice()}
                >
                    <button
                        className="usage-notice-close"
                        onClick={() => void closeUsageNotice()}
                        aria-label={t("modal.close")}
                    >
                        <span className="ms" aria-hidden="true">close</span>
                    </button>
                    <div className="usage-notice-shell" onClick={(event) => event.stopPropagation()}>
                        <div className="usage-notice-mark" aria-hidden="true">
                            <span className="ms">info</span>
                        </div>

                        <div className="usage-notice-copy">
                            <h2 id="usage-notice-title">{t("app.usage_notice.title")}</h2>
                            <p>{t("app.usage_notice.intro")}</p>
                            <p>{t("app.usage_notice.open_source")}</p>
                            <p>{t("app.usage_notice.risk")}</p>
                        </div>

                        <div className="usage-notice-terms">
                            <section>
                                <h3>{t("app.usage_notice.cn_terms_title")}</h3>
                                <p>{t("app.usage_notice.cn_terms_body")}</p>
                            </section>
                            <section>
                                <h3>{t("app.usage_notice.jp_terms_title")}</h3>
                                <p>{t("app.usage_notice.jp_terms_body")}</p>
                            </section>
                            <p className="usage-notice-terms-note">{t("app.usage_notice.terms_note")}</p>
                        </div>

                        <p className="usage-notice-disclaimer">{t("app.usage_notice.disclaimer")}</p>

                        <label className="usage-notice-check">
                            <input
                                type="checkbox"
                                checked={usageNotice.checked}
                                onChange={(event) => {
                                    const checked = event.currentTarget.checked;
                                    toggleUsageNoticeAcknowledgement(checked);
                                }}
                            />
                            <span>{t("app.usage_notice.ack_label")}</span>
                        </label>

                        <button
                            className={`usage-notice-action ${usageNotice.checked ? "is-continue" : "is-close"}`}
                            onClick={() => void submitUsageNotice()}
                        >
                            <span className="ms" aria-hidden="true">
                                {usageNotice.checked ? "check_circle" : "power_settings_new"}
                            </span>
                            {usageNotice.checked ? t("common.continue") : t("app.usage_notice.close_app")}
                        </button>
                    </div>
                </div>
            ) : null}
            {versionMismatch ? (
                <div
                    className="version-mismatch-overlay"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="version-mismatch-title"
                    onClick={() => setVersionMismatch(null)}
                >
                    <button
                        className="version-mismatch-close"
                        onClick={() => setVersionMismatch(null)}
                        aria-label={t("modal.close")}
                    >
                        <span className="ms" aria-hidden="true">close</span>
                    </button>
                    <div className="version-mismatch-shell" onClick={(event) => event.stopPropagation()}>
                        <div className="version-mismatch-mark" aria-hidden="true">
                            <span className="ms">warning</span>
                        </div>

                        <div className="version-mismatch-copy">
                            <h2 id="version-mismatch-title">{t("app.version_mismatch.title")}</h2>
                            <p>{t("app.version_mismatch.message")}</p>
                        </div>

                        <div className="version-mismatch-grid">
                            <div className="version-mismatch-card">
                                <span className="ms" aria-hidden="true">desktop_windows</span>
                                <div>
                                    <div className="version-mismatch-label">{t("app.version_mismatch.frontend_label")}</div>
                                    <div className="version-mismatch-value">{versionMismatch.frontendVersion || APP_VERSION}</div>
                                </div>
                            </div>
                            <div className="version-mismatch-card is-backend">
                                <span className="ms" aria-hidden="true">dns</span>
                                <div>
                                    <div className="version-mismatch-label">{t("app.version_mismatch.backend_label")}</div>
                                    <div className="version-mismatch-value">{versionMismatch.backendVersion || "unknown"}</div>
                                </div>
                            </div>
                        </div>

                        <p className="version-mismatch-hint">{t("app.version_mismatch.hint")}</p>
                    </div>
                </div>
            ) : null}
            {updateDialog ? (
                <div
                    className="update-dialog-overlay"
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="update-dialog-title"
                    onClick={() => setUpdateDialog(null)}
                >
                    <button
                        className="update-dialog-close"
                        onClick={() => setUpdateDialog(null)}
                        aria-label={t("modal.close")}
                    >
                        <span className="ms" aria-hidden="true">close</span>
                    </button>
                    <div className="update-dialog-shell" onClick={(event) => event.stopPropagation()}>
                        <div className="update-dialog-mark" aria-hidden="true">
                            <span className="ms">system_update_alt</span>
                        </div>

                        <div className="update-dialog-copy">
                            <h2 id="update-dialog-title">{t("update.dialog_title")}</h2>
                            <p>{t("update.dialog_body", {version: updateDialog.update.version})}</p>
                        </div>

                        <div className="update-dialog-meta">
                            <div>
                                <span>{t("update.current_version")}</span>
                                <strong>v{APP_VERSION}</strong>
                            </div>
                            <div>
                                <span>{t("update.latest_version")}</span>
                                <strong>v{updateDialog.update.version}</strong>
                            </div>
                            <div>
                                <span>{t("update.download_package")}</span>
                                <strong>{updateDialog.update.downloadAssetName || t("update.release_page")}</strong>
                            </div>
                        </div>

                        {updateDialog.update.body ? (
                            <pre className="update-dialog-notes">{updateDialog.update.body}</pre>
                        ) : null}

                        <label className={`update-dialog-check ${updateDialog.autoCheck ? "is-on" : ""}`}>
                            <input
                                type="checkbox"
                                checked={updateDialog.autoCheck}
                                onChange={(event) => {
                                    const autoCheck = event.currentTarget.checked;
                                    setUpdateAutoCheck(autoCheck);
                                    setUpdateDialog((current) => current ? {...current, autoCheck} : current);
                                }}
                            />
                            <span>{t("update.auto_check_label")}</span>
                        </label>
                        <div className="update-dialog-actions">
                            <button
                                className="nav-btn"
                                onClick={() => {
                                    ignoreUpdateVersion(updateDialog.update.version);
                                    setUpdateDialog(null);
                                }}
                            >
                                {t("update.ignore_version")}
                            </button>
                            <button className="nav-btn" onClick={() => void openUpdateUrl(updateDialog.update.releaseUrl)}>
                                {t("update.open_release")}
                            </button>
                            <button className="nav-btn update-dialog-primary" onClick={() => void openUpdateUrl(updateDialog.update.downloadUrl)}>
                                {t("update.download_update")}
                            </button>
                        </div>
                    </div>
                </div>
            ) : null}
            {souzuSwitchExecution ? createPortal(
                <SouzuSwitchExecutionOverlay
                    execution={souzuSwitchExecution}
                    onConfirm={() => setSouzuSwitchExecution(null)}
                />,
                document.body,
            ) : null}
        </div>
    );
}

function SouzuSwitchExecutionOverlay({
                                         execution,
                                         onConfirm,
                                     }: {
    execution: SouzuSwitchExecutionState;
    onConfirm: () => void;
}) {
    const {t} = useTranslation();
    const finished = execution.status !== "running";
    const [autoCloseSeconds, setAutoCloseSeconds] = React.useState(3);
    const isFullPlan = execution.execution_kind === "full_plan";
    const title = execution.status === "failed"
        ? t(isFullPlan ? "blackhole.execute_full_failed_title" : "blackhole.execute_failed_title")
        : finished
            ? t(isFullPlan ? "blackhole.execute_full_completed_title" : "blackhole.execute_completed_title")
            : t(isFullPlan ? "blackhole.execute_full_progress_title" : "blackhole.execute_progress_title");
    const body = execution.status === "failed"
        ? t(isFullPlan ? "blackhole.execute_full_failed_body" : "blackhole.execute_failed_body")
        : finished
            ? t(isFullPlan ? "blackhole.execute_full_completed_body" : "blackhole.execute_completed_body")
            : t(isFullPlan ? "blackhole.execute_full_progress_body" : "blackhole.execute_progress_body");
    const phaseText = execution.phase_key
        ? t(execution.phase_key, execution.reason_values)
        : execution.phase;
    const reasonText = execution.reason_key
        ? t(execution.reason_key, execution.reason_values)
        : execution.reason;
    React.useEffect(() => {
        if (!finished) {
            setAutoCloseSeconds(3);
            return;
        }
        setAutoCloseSeconds(3);
        const interval = window.setInterval(() => {
            setAutoCloseSeconds((value) => Math.max(0, value - 1));
        }, 1000);
        const timeout = window.setTimeout(onConfirm, 3000);
        return () => {
            window.clearInterval(interval);
            window.clearTimeout(timeout);
        };
    }, [finished, onConfirm, execution.status, execution.updated_at]);
    return (
        <div
            className={`usage-notice-overlay blackhole-execute-progress-overlay ${finished ? "is-finished" : ""} ${execution.status === "failed" ? "is-failed" : ""}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="blackhole-execute-progress-title"
        >
            <div className="usage-notice-shell blackhole-execute-progress-shell">
                <div className="usage-notice-mark" aria-hidden="true">
                    <span className="ms">{execution.status === "failed" ? "error" : finished ? "check_circle" : "sync"}</span>
                </div>

                <div className="usage-notice-copy">
                    <h2 id="blackhole-execute-progress-title">{title}</h2>
                    <p>{body}</p>
                    {execution.batch_count > 0 ? (
                        <p>{t("blackhole.execute_progress_batches", {
                            count: execution.batch_count,
                            current: Math.min(execution.batch_index || 1, execution.batch_count),
                        })}</p>
                    ) : null}
                    {!finished && phaseText ? <p>{phaseText}</p> : null}
                    {reasonText ? <p>{t("blackhole.execute_result_reason", {reason: reasonText})}</p> : null}
                </div>

                {finished ? (
                    <button className="usage-notice-action is-continue" onClick={onConfirm}>
                        <span className="ms" aria-hidden="true">check_circle</span>
                        {t("common.ok")} ({autoCloseSeconds})
                    </button>
                ) : (
                    <div className="blackhole-execute-progress" role="progressbar" aria-label={title}>
                        <span/>
                    </div>
                )}
            </div>
        </div>
    );
}
