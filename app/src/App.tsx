import React from "react";
import "./styles/theme.css";
import "./App.css";
import {listen} from "@tauri-apps/api/event";
import {invoke} from "@tauri-apps/api/core";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import PacketTestPage from "./pages/PacketTestPage";
import FrontendTestPage from "./pages/FrontendTestPage";
import AutoRunnerPage from "./pages/AutoRunnerPage";
import FusePage from "./pages/FusePage";
import AboutPage from "./pages/AboutPage";
import BlackHolePage from "./pages/BlackHolePage";
import SouzuSwitchDebugPage from "./pages/SouzuSwitchDebugPage";
import ScorePage from "./pages/ScorePage";
import {ws, ensureWsStartedOnce} from "./lib/ws";
import {type LogLevel, useLogStore} from "./lib/logStore";
import TileGrid from "./components/TileGrid";
import WallStats from "./components/WallStats";
import ReplacementPanel from "./components/ReplacementPanel";
import ReplacementStats from "./components/ReplacementStats";
import AdvisorPanel from "./components/AdvisorPanel";
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
    type WsEnvelope,
} from "./lib/gamestate";
import {installWsToastBridge, pushToast, useGlobalToast} from "./lib/toast";
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

type Route = "home" | "score" | "blackhole" | "souzu-debug" | "fuse" | "autorun" | "settings" | "diagnostics" | "packet-test" | "frontend-test" | "about";
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
        || route === "souzu-debug"
        || route === "diagnostics"
        || route === "frontend-test"
        || route === "packet-test"
        || route === "about";
}

const OUTER_PADDING = 16;
const MAIN_GAP = 12;
const SHOP_BUFF_EXCHANGE_ID = 8001;
const SHOP_BUFF_UPGRADE_COSTS = [5, 10, 15, 20, 50, 100, 150, 200];

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
                }: {
    onSecretClick: () => void;
    stage: number;
    point?: string;
    targetPoint?: string;
    onTutorialClick?: () => void;
}) {
    const {t} = useTranslation();
    const progressMeta = (stage === 2 || stage === 3) ? buildPointProgressMeta(point, targetPoint) : null;
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
        if (!appWindow) return;
        try {
            await appWindow.close();
        } catch (e) {
            console.error("close failed", e);
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
                <span className="title" onClick={onSecretClick}>{t("app.title")}</span>
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

export default function App() {
    const {t} = useTranslation();
    type ThemeMode = "auto" | "dark" | "dark-green" | "dark-purple";
    const {toast, visible: toastVisible} = useGlobalToast();
    const {config: autoConfig, status: autoStatus} = useAutoRunner();
    const [route, setRoute] = React.useState<Route>("home");
    const sidebarRef = React.useRef<HTMLDivElement | null>(null);
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
    const [activeTutorial, setActiveTutorial] = React.useState<TutorialId | null>(null);

    const [cells, setCells] = React.useState<Cell[]>([]);
    const [stage, setStage] = React.useState<number>(0);
    const [coin, setCoin] = React.useState<string>("0");
    const [point, setPoint] = React.useState<string>("0");
    const [targetPoint, setTargetPoint] = React.useState<string>("0");
    const [level, setLevel] = React.useState<number>(0);
    const [ended, setEnded] = React.useState<boolean>(false);
    const [remain, setRemain] = React.useState<number>(0);
    const [hasGame, setHasGame] = React.useState<boolean>(false);
    const [bossBuff, setBossBuff] = React.useState<number[]>([]);
    const [shopBuffList, setShopBuffList] = React.useState<Record<number, number>>({});

    const [wallStatsTiles, setWallStatsTiles] = React.useState<string[]>([]);
    const [handTileIds, setHandTileIds] = React.useState<number[]>([]);

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
            title: t("tutorial.home.step_refresh.title"),
            body: t("tutorial.home.step_refresh.body"),
            targetSelector: '[data-tutorial="nav-refresh"]',
        },
    ], [t]);

    const blackHoleTutorialSteps = React.useMemo<TutorialStep[]>(() => [
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
    const [debugSouzuSwitch, setDebugSouzuSwitch] = React.useState<PlanData | null>(null);
    const [latestGameState, setLatestGameState] = React.useState<GameStateData | null>(null);

    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);
    const [goods, setGoods] = React.useState<GoodsItem[]>([]);
    const [candidates, setCandidates] = React.useState<CandidateEffectRef[]>([]);
    const [tileScoreMap, setTileScoreMap] = React.useState<Record<string, string>>({});

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
            await getCurrentWindow().close();
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

    const handleUpgradeExchangeShopBuff = React.useCallback(() => {
        if (stage !== 4) {
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
        ws.send({
            type: "upgrade_shop_buff",
            data: {activityId: 250811, id: SHOP_BUFF_EXCHANGE_ID},
        } as any);
    }, [coin, nextExchangeShopBuffCost, stage, t]);

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
        if (stage === 2) return;
        if (stage === 3) {
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
                    label: "正在完成界面初始化",
                    detail: "准备显示主窗口",
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
        ensureWsStartedOnce();
        setConnected(ws.connected);
        installWsToastBridge(ws);
        const offOpen = ws.onOpen(() => setConnected(true));
        const offClose = ws.onClose(() => setConnected(false));

        const offPkt = ws.onPacket((pkt: WsEnvelope) => {
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
                setEnded(d.ended);
                setRemain(d.desktop_remain ?? 0);
                setHasGame(d.stage !== undefined && d.ended !== undefined && d.stage >= 0);
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

                if (!(d.stage === 2 || d.stage === 3)) {
                    setPlanSuuAnkou(null);
                    setPlanChiitoi(null);
                }

                setAmulets(Array.isArray(d.effect_list) ? d.effect_list : []);
                setGoods(d.goods ?? []);
                setCandidates(d.candidate_effect_list ?? []);
                setTileScoreMap(d.tile_score_map ?? {});
            } else if (pkt.type === "discard_recommendation" && pkt.data) {
                const arr = (Array.isArray(pkt.data) ? pkt.data : []) as Array<{ yaku: string; data: PlanData }>;
                for (const item of arr) {
                    if (!item || !item.yaku) continue;
                    if (item.yaku === "chiitoi") setPlanChiitoi(item.data ?? null);
                    else if (item.yaku === "suuannkou") setPlanSuuAnkou(item.data ?? null);
                    else if (item.yaku === "souzu_switch") {
                        const source = (item.data as any)?.request_source;
                        if (source === "debug") setDebugSouzuSwitch(item.data ?? null);
                        else setPlanSouzuSwitch(item.data ?? null);
                    }
                }
            } else if (pkt.type === "autorun_status" && pkt.data) {
                setAutoStatus(pkt.data as AutoRunnerStatus);
            } else if (pkt.type === "upgrade_shop_buff_result") {
                const d = (pkt.data ?? {}) as {
                    ok?: boolean;
                    reason?: string;
                    cost?: number;
                    coin?: number;
                };
                if (d.ok) {
                    pushToast(t("shop_buff_upgrade.success", {name: t("shop_buff_upgrade.exchange_name")}), "success", 1800);
                } else if (d.reason === "insufficient_coin") {
                    pushToast(t("shop_buff_upgrade.insufficient_coin", {
                        cost: d.cost ?? 0,
                        coin: d.coin ?? 0,
                    }), "error", 2200);
                } else if (d.reason === "maxed") {
                    pushToast(t("shop_buff_upgrade.maxed", {name: t("shop_buff_upgrade.exchange_name")}), "info", 1800);
                } else if (d.reason === "addon-not-ready") {
                    pushToast(t("shop_buff_upgrade.addon_not_ready"), "error", 2200);
                } else {
                    pushToast(t("shop_buff_upgrade.failed", {reason: d.reason || "unknown"}), "error", 2600);
                }
            } else if (pkt.type === "version_mismatch" && pkt.data) {
                const d = pkt.data as Partial<VersionMismatch>;
                if (!versionMismatchShownRef.current) {
                    versionMismatchShownRef.current = true;
                    setVersionMismatch({
                        frontendVersion: String(d.frontendVersion || APP_VERSION),
                        backendVersion: String(d.backendVersion || "unknown"),
                    });
                }
                return;
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
        });

        const addLog = useLogStore.getState().addLog;
        const addLogs = useLogStore.getState().addLogs;
        let unsubs: Array<() => void> = [];
        (async () => {
            const chunkBuffers = new Map<string, { total: number; parts: string[] }>();

            const handleBackendLogPayload = (event: string, level: LogLevel, payload: BackendLogPayload) => {
                if (typeof payload === "string") {
                    addLog(level, `${event}: ${payload}`);
                    return;
                }

                if (Array.isArray(payload)) {
                    addLogs(level, payload.map((line) => `${event}: ${line}`));
                    return;
                }

                if (!payload || typeof payload !== "object") return;

                if (payload.kind === "lines" && Array.isArray(payload.lines)) {
                    addLogs(level, payload.lines.map((line) => `${event}: ${line}`));
                    return;
                }

                if (payload.kind === "chunk" && typeof payload.id === "number" && typeof payload.total === "number") {
                    const key = `${event}:${payload.id}`;
                    const bucket = chunkBuffers.get(key) ?? {
                        total: payload.total,
                        parts: Array.from({length: payload.total}, () => ""),
                    };
                    bucket.total = payload.total;
                    if (typeof payload.index === "number" && payload.index >= 0 && payload.index < bucket.parts.length) {
                        bucket.parts[payload.index] = payload.text ?? "";
                    }
                    chunkBuffers.set(key, bucket);
                    if (bucket.parts.every((part) => part !== "")) {
                        chunkBuffers.delete(key);
                        addLog(level, `${event}: ${bucket.parts.join("")}`);
                    }
                }
            };

            const sub = async (event: string, level: LogLevel = "INFO") => {
                const un = await listen<BackendLogPayload>(event, (e) => {
                    handleBackendLogPayload(event, level, e.payload);
                });
                unsubs.push(un);
            };
            await sub("backend:spawn", "INFO");
            await sub("backend:ready", "INFO");
            await sub("backend:stdout", "STDOUT");
            await sub("backend:stderr", "STDERR");
            await sub("backend:exit", "WARN");
            await sub("backend:error", "ERROR");
        })();

        return () => {
            offOpen();
            offClose();
            offPkt();
            unsubs.forEach((u) => u());
            unsubs = [];
        };
    }, []);

    React.useEffect(() => {
        if (!debugEnabled && (route === "packet-test" || route === "souzu-debug")) {
            setRoute("diagnostics");
        }
    }, [debugEnabled, route]);

    React.useEffect(() => {
        let un = () => {
        };
        (async () => {
            un = await listen<{ lng: string }>("i18n:set-language", (e) => {
                setAppLanguage(e.payload.lng);
            });
        })();
        return () => un();
    }, []);

    const statsHeader = stage === 2 ? (
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
                    <button ref={(el) => {
                        navRefs.current.autorun = el;
                    }} className={`nav-icon ${route === "autorun" ? "active" : ""}`} data-tutorial="nav-autorun" title={t("nav.autorun")} onClick={() => setRoute("autorun")}>
                        <span className="ms">autoplay</span>
                    </button>
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
                                        <span>前端测试</span>
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
                                {debugEnabled && (
                                    <button
                                        className={`more-menu-item ${route === "packet-test" ? "active" : ""}`}
                                        role="menuitem"
                                        onClick={() => navigateFromMore("packet-test")}
                                    >
                                        <span className="ms">send_and_archive</span>
                                        <span>{t("nav.packetTest")}</span>
                                    </button>
                                )}
                            </div>
                        )}
                    </div>

                    <div className="sidebar-spacer"/>

                    <div className="sidebar-bottom">
                        <button
                            className="nav-icon"
                            data-tutorial="nav-refresh"
                            title={t("nav.refreshGame")}
                            onClick={() => ws.send({type: "fetch_amulet_activity_data", data: {activityId: 250811}})}
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

                <main className="main-pane">
                    <div
                        className={`app-main route-${route}`}
                        style={{padding: `${OUTER_PADDING}px ${OUTER_PADDING}px 0 ${OUTER_PADDING}px`, boxSizing: "border-box"}}
                    >
                        {route === "home" && (
                            <div
                                className="home-grid"
                                style={{display: "flex", alignItems: "stretch", gap: MAIN_GAP}}
                            >
                                {(stage === 2 || stage === 3) && (
                                    <div className="panel advisor home-advisor-panel">
                                        <AdvisorPanel
                                            suuAnkou={planSuuAnkou}
                                            chiitoi={planChiitoi}
                                            resolveFace={(id) => deckMap.get(id) ?? null}
                                        />
                                    </div>
                                )}

                                <div style={{flex: 1, minWidth: 0, position: "relative"}}>
                                    <div className="panel">
                                        <div className="panel-title">{t("amulet")}</div>
                                        <AmuletBar items={amulets} scale={0.55}/>
                                    </div>

                                    {(stage === 4 || stage === 5) && (
                                        <div className="panel">
                                            <div className="panel-title">{t("goods")}</div>
                                            <GoodsBar items={goods} scale={0.85}/>
                                        </div>
                                    )}

                                    {stage === 4 && (
                                        <div className="panel">
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

                                    {[1, 5, 7].includes(stage) && (
                                        <div className="panel">
                                            <div className="panel-title">{t("candidate_amulet")}</div>
                                            <CandidateBar candidates={candidates} scale={0.55}/>
                                        </div>
                                    )}

                                    {(stage === 2 || stage === 3) && (
                                        <TileGrid
                                            cells={cells}
                                            tianDoraTiles={tianDoraTiles}
                                            doraCountByTile={doraCountByTile}
                                        />
                                    )}

                                    {stage === 2 && replacementTiles.length > 0 && (
                                        <ReplacementPanel replacementTiles={replacementTiles} usedCount={switchUsedCount}/>
                                    )}

                                </div>

                                {(stage === 2 || stage === 3) && (
                                    <div className="right-side-panel">
                                        {stage === 2 && rightPanelMode === "replacementStats" ? (
                                            <ReplacementStats replacementTiles={replacementTiles} usedCount={switchUsedCount} headerSlot={statsHeader}/>
                                        ) : (
                                            <WallStats wallTiles={wallStatsTiles} headerSlot={statsHeader}/>
                                        )}
                                    </div>
                                )}
                            </div>
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
                        {route === "souzu-debug" && (
                            <SouzuSwitchDebugPage
                                currentState={latestGameState}
                                data={debugSouzuSwitch}
                                onClear={() => setDebugSouzuSwitch(null)}
                            />
                        )}
                        {route === "autorun" && <AutoRunnerPage/>}
                        {route === "settings" && <SettingsPage/>}
                        {route === "diagnostics" && <DiagnosticsPage/>}
                        {route === "frontend-test" && <FrontendTestPage/>}
                        {route === "packet-test" && debugEnabled && <PacketTestPage/>}
                        {route === "about" && (
                            <AboutPage
                                onSecretClick={onSecretClick}
                                onShowUsageNotice={openUsageNotice}
                            />
                        )}
                    </div>
                </main>
            </div>

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
                            <span className={`badge ${ended ? "down" : "ok"}`}>{ended ? t("status.ended") : t("status.running")}</span>
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
        </div>
    );
}
