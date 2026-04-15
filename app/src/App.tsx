import React from "react";
import "./styles/theme.css";
import "./App.css";
import {listen} from "@tauri-apps/api/event";
import {invoke} from "@tauri-apps/api/core";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import PacketTestPage from "./pages/PacketTestPage";
import AutoRunnerPage from "./pages/AutoRunnerPage";
import FusePage from "./pages/FusePage";
import AboutPage from "./pages/AboutPage";
import BlackHolePage from "./pages/BlackHolePage";
import SouzuSwitchDebugPage from "./pages/SouzuSwitchDebugPage";
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
import {installWsToastBridge, useGlobalToast} from "./lib/toast";
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
import type {PlanData, SearchRuntimeData} from "./lib/planTypes";

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

type Route = "home" | "blackhole" | "souzu-debug" | "fuse" | "autorun" | "settings" | "diagnostics" | "packet-test" | "about";

const OUTER_PADDING = 16;
const SIDEBAR_WIDTH = 320;
const MAIN_GAP = 12;

const appWindow = getCurrentWindow();

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

function Topbar({
    onSecretClick,
    stage,
    point,
    targetPoint,
}: {
    onSecretClick: () => void;
    stage: number;
    point?: string;
    targetPoint?: string;
}) {
    const {t} = useTranslation();
    const progressMeta = (stage === 2 || stage === 3) ? buildPointProgressMeta(point, targetPoint) : null;
    const onMin = async () => {
        try {
            await appWindow.minimize();
        } catch (e) {
            console.error("minimize failed", e);
        }
    };
    const onTgl = async () => {
        try {
            await appWindow.toggleMaximize();
        } catch (e) {
            console.error("toggleMaximize failed", e);
        }
    };
    const onClose = async () => {
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
                        <div className="topbar-progress-band-fill" style={{width: `${progressMeta.fillPercent}%`}} />
                    </div>
                    <div className="topbar-progress-band-label" data-tauri-drag-region>{progressMeta.centerLabel}</div>
                </div>
            ) : null}
            <div className="topbar-left drag" data-tauri-drag-region>
                <span className="title" onClick={onSecretClick}>{t("app.title")}</span>
            </div>

            <div className="win" data-tauri-drag-region="false">
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
    const themeButtonRef = React.useRef<HTMLButtonElement | null>(null);
    const [connected, setConnected] = React.useState(false);
    const [debugEnabled, setDebugEnabled] = React.useState(false);

    const [cells, setCells] = React.useState<Cell[]>([]);
    const [stage, setStage] = React.useState<number>(0);
    const [coin, setCoin] = React.useState<string>("0");
    const [point, setPoint] = React.useState<string>("0");
    const [targetPoint, setTargetPoint] = React.useState<string>("0");
    const [ended, setEnded] = React.useState<boolean>(false);
    const [remain, setRemain] = React.useState<number>(0);
    const [hasGame, setHasGame] = React.useState<boolean>(false);
    const [bossBuff, setBossBuff] = React.useState<number[]>([]);

    const [wallStatsTiles, setWallStatsTiles] = React.useState<string[]>([]);
    const [handTileIds, setHandTileIds] = React.useState<number[]>([]);

    const [replacementTiles, setReplacementTiles] = React.useState<string[]>([]);
    const [replacementTileIds, setReplacementTileIds] = React.useState<number[]>([]);
    const [switchUsedCount, setSwitchUsedCount] = React.useState<number>(0);
    const [rightPanelMode, setRightPanelMode] = React.useState<"replacementStats" | "wall">("replacementStats");
    const [wallTileIds, setWallTileIds] = React.useState<number[]>([]);

    const [deckMap, setDeckMap] = React.useState<Map<number, string>>(new Map());

    const [planSuuAnkou, setPlanSuuAnkou] = React.useState<PlanData | null>(null);
    const [planChiitoi, setPlanChiitoi] = React.useState<PlanData | null>(null);
    const [planSouzuSwitch, setPlanSouzuSwitch] = React.useState<PlanData | null>(null);
    const [debugSouzuSwitch, setDebugSouzuSwitch] = React.useState<PlanData | null>(null);
    const [souzuSwitchRuntime, setSouzuSwitchRuntime] = React.useState<SearchRuntimeData | null>(null);
    const [latestGameState, setLatestGameState] = React.useState<GameStateData | null>(null);

    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);
    const [goods, setGoods] = React.useState<GoodsItem[]>([]);
    const [candidates, setCandidates] = React.useState<CandidateEffectRef[]>([]);

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
                setCoin(typeof d.coin === "string" ? d.coin : String(d.coin ?? "0"));
                setPoint(typeof d.point === "string" ? d.point : String(d.point ?? "0"));
                setTargetPoint(typeof d.target_point === "string" ? d.target_point : String(d.target_point ?? "0"));
                setEnded(!!d.ended);
                setRemain(d.desktop_remain ?? 0);
                setHasGame(d.stage !== undefined && d.ended !== undefined && d.stage >= 0);
                setBossBuff(Array.isArray((d as any).boss_buff) ? (d as any).boss_buff : []);

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
            } else if (pkt.type === "discard_recommendation" && pkt.data) {
                const arr = (Array.isArray(pkt.data) ? pkt.data : []) as Array<{ yaku: string; data: PlanData }>;
                for (const item of arr) {
                    if (!item || !item.yaku) continue;
                    if (item.yaku === "chiitoi") setPlanChiitoi(item.data ?? null);
                    else if (item.yaku === "suuannkou") setPlanSuuAnkou(item.data ?? null);
                    else if (item.yaku === "souzu_switch") {
                        if ((item.data as any)?.runtime) setSouzuSwitchRuntime((item.data as any).runtime ?? null);
                        const source = (item.data as any)?.request_source;
                        if (source === "debug") setDebugSouzuSwitch(item.data ?? null);
                        else setPlanSouzuSwitch(item.data ?? null);
                    }
                }
            } else if (pkt.type === "souzu_switch_runtime") {
                setSouzuSwitchRuntime((pkt.data as SearchRuntimeData) ?? null);
            } else if (pkt.type === "autorun_status" && pkt.data) {
                setAutoStatus(pkt.data as AutoRunnerStatus);
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
        if (!debugEnabled && route === "packet-test") {
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

    React.useEffect(() => {
        const updateSidebarIndicator = () => {
            const sidebar = sidebarRef.current;
            const activeBtn = navRefs.current[route];
            if (!sidebar || !activeBtn) return;

            // Use layout offsets so entrance transforms don't skew initial indicator position.
            const top = activeBtn.offsetTop;
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

        window.addEventListener("resize", updateSidebarIndicator);
        return () => {
            ro.disconnect();
            window.removeEventListener("resize", updateSidebarIndicator);
        };
    }, [route]);

    return (
        <div className="app">
            <div className="app-ambient" aria-hidden="true">
                <span className="ambient-orb ambient-orb-a"/>
                <span className="ambient-orb ambient-orb-b"/>
                <span className="ambient-grid"/>
            </div>
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>

            <Topbar onSecretClick={onSecretClick} stage={stage} point={point} targetPoint={targetPoint}/>

            <div className="shell">
                <aside className="sidebar" ref={sidebarRef}>
                    <div className="sidebar-active-indicator" aria-hidden="true"/>
                    <button ref={(el) => {
                        navRefs.current.home = el;
                    }} className={`nav-icon ${route === "home" ? "active" : ""}`} title={t("nav.home")} onClick={() => setRoute("home")}>
                        <span className="ms">home</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.fuse = el;
                    }} className={`nav-icon ${route === "fuse" ? "active" : ""}`} title={t("nav.fuse")} onClick={() => setRoute("fuse")}>
                        <span className="ms">gpp_maybe</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.blackhole = el;
                    }} className={`nav-icon ${route === "blackhole" ? "active" : ""}`} title={t("nav.blackhole")} onClick={() => setRoute("blackhole")}>
                        <span className="ms">deblur</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current["souzu-debug"] = el;
                    }} className={`nav-icon ${route === "souzu-debug" ? "active" : ""}`} title="Souzu Debug" onClick={() => setRoute("souzu-debug")}>
                        <span className="ms">science</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.autorun = el;
                    }} className={`nav-icon ${route === "autorun" ? "active" : ""}`} title={t("nav.autorun")} onClick={() => setRoute("autorun")}>
                        <span className="ms">autoplay</span>
                    </button>
                    <button ref={(el) => {
                        navRefs.current.diagnostics = el;
                    }} className={`nav-icon ${route === "diagnostics" ? "active" : ""}`} title={t("nav.diagnostics")} onClick={() => setRoute("diagnostics")}>
                        <span className="ms">article</span>
                    </button>
                    {debugEnabled && (
                        <button ref={(el) => {
                            navRefs.current["packet-test"] = el;
                        }} className={`nav-icon ${route === "packet-test" ? "active" : ""}`} title={t("nav.packetTest")} onClick={() => setRoute("packet-test")}>
                            <span className="ms">send_and_archive</span>
                        </button>
                    )}
                    <button ref={(el) => {
                        navRefs.current.about = el;
                    }} className={`nav-icon ${route === "about" ? "active" : ""}`} title={t("nav.about")} onClick={() => setRoute("about")}>
                        <span className="ms">help</span>
                    </button>

                    <div className="sidebar-spacer"/>

                    <div className="sidebar-bottom">
                        <button
                            className="nav-icon"
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
                                    <div className="panel advisor" style={{width: SIDEBAR_WIDTH, flex: "0 0 auto"}}>
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

                                    {[1, 5, 7].includes(stage) && (
                                        <div className="panel">
                                            <div className="panel-title">{t("candidate_amulet")}</div>
                                            <CandidateBar candidates={candidates} scale={0.55}/>
                                        </div>
                                    )}

                                    {(stage === 2 || stage === 3) && <TileGrid cells={cells}/>}

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
                                runtime={souzuSwitchRuntime}
                                onClear={() => setPlanSouzuSwitch(null)}
                            />
                        )}
                        {route === "souzu-debug" && (
                            <SouzuSwitchDebugPage
                                currentState={latestGameState}
                                data={debugSouzuSwitch}
                                runtime={souzuSwitchRuntime}
                                onClear={() => setDebugSouzuSwitch(null)}
                            />
                        )}
                        {route === "autorun" && <AutoRunnerPage/>}
                        {route === "settings" && <SettingsPage/>}
                        {route === "diagnostics" && <DiagnosticsPage/>}
                        {route === "packet-test" && debugEnabled && <PacketTestPage/>}
                        {route === "about" && <AboutPage onSecretClick={onSecretClick}/>}
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
        </div>
    );
}
