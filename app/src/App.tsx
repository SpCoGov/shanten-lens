import React from "react";
import "./styles/theme.css";
import "./App.css";
import {listen} from "@tauri-apps/api/event";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
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
import {AutoRunnerStatus, setAutoStatus} from "./lib/autoRunnerStore";
import GoodsBar from "./components/GoodsBar";
import CandidateBar from "./components/CandidateBar";
import "./fonts/material-symbols.css";
import {getCurrentWindow} from "@tauri-apps/api/window";
import "./lib/i18n";
import {useTranslation} from "react-i18next";
import {WebviewWindow, getAllWebviewWindows} from '@tauri-apps/api/webviewWindow';
import {t} from "i18next";
import {openMsgBoxWindow} from "./lib/msgbox";
import type {PlanData} from "./lib/planTypes";

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

type Route = "home" | "blackhole" | "souzu-debug" | "fuse" | "autorun" | "settings" | "diagnostics" | "about";

const OUTER_PADDING = 16;
const SIDEBAR_WIDTH = 320;
const MAIN_GAP = 12;

const appWindow = getCurrentWindow();

function Topbar() {
    const {t} = useTranslation();
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
        <header className="topbar">
            <div className="drag" data-tauri-drag-region>
                <span className="title">{t("app.title")}</span>
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
    const {toast, visible: toastVisible} = useGlobalToast();
    const [route, setRoute] = React.useState<Route>("home");
    const [connected, setConnected] = React.useState(false);

    const [cells, setCells] = React.useState<Cell[]>([]);
    const [stage, setStage] = React.useState<number>(0);
    const [coin, setCoin] = React.useState<number>(0);
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
    const [latestGameState, setLatestGameState] = React.useState<GameStateData | null>(null);

    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);
    const [goods, setGoods] = React.useState<GoodsItem[]>([]);
    const [candidates, setCandidates] = React.useState<CandidateEffectRef[]>([]);

    type ThemeMode = "auto" | "dark" | "dark-green";
    const THEME_ORDER: ThemeMode[] = ["auto", "dark", "dark-green"];
    const THEME_KEY = "sl-theme";

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

        if (t === "dark" || t === "dark-green") {
            root.setAttribute("data-theme", t);
        }
    }

    const [theme, setTheme] = React.useState<ThemeMode>(() => {
        const saved = localStorage.getItem(THEME_KEY) as ThemeMode | null;
        if (saved === "auto" || saved === "dark" || saved === "dark-green") {
            return saved;
        }
        if (saved === "dark") return "dark";
        return "auto";
    });

    React.useEffect(() => {
        applyTheme(theme);
        localStorage.setItem(THEME_KEY, theme);
    }, [theme]);

    const themeIcon = (() => {
        switch (theme) {
            case "auto":
                return "light_mode";
            case "dark":
                return "dark_mode";
            case "dark-green":
                return "forest";
            default:
                return "light_mode";
        }
    })();

    const themeLabel = (() => {
        return t("app.theme." + theme);
    })();

    const nextTheme = React.useCallback((t: ThemeMode): ThemeMode => {
        const i = THEME_ORDER.indexOf(t);
        return THEME_ORDER[(i + 1) % THEME_ORDER.length];
    }, []);

    const toggleTheme = () => setTheme((prev) => nextTheme(prev));

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
        ensureWsStartedOnce();
        setConnected(ws.connected);
        installWsToastBridge(ws);
        const offOpen = ws.onOpen(() => setConnected(true));
        const offClose = ws.onClose(() => setConnected(false));

        const offPkt = ws.onPacket((pkt: WsEnvelope) => {
            if (pkt.type === "update_gamestate") {
                const d = pkt.data as GameStateData;
                setLatestGameState(d);
                const deck = toDeckMap(d.deck_map);
                setDeckMap(deck);
                const list = buildCells(deck, d.locked_tiles ?? [], d.wall_tiles ?? [], 36);
                setCells(list);
                setStage(d.stage ?? 0);
                setCoin(d.coin ?? 0);
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
                        const source = (item.data as any)?.request_source;
                        if (source === "debug") setDebugSouzuSwitch(item.data ?? null);
                        else setPlanSouzuSwitch(item.data ?? null);
                    }
                }
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

    return (
        <div className="app">
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>

            <Topbar/>

            <div className="shell">
                <aside className="sidebar">
                    <button className={`nav-icon ${route === "home" ? "active" : ""}`} title={t("nav.home")} onClick={() => setRoute("home")}>
                        <span className="ms">home</span>
                    </button>
                    <button className={`nav-icon ${route === "fuse" ? "active" : ""}`} title={t("nav.fuse")} onClick={() => setRoute("fuse")}>
                        <span className="ms">gpp_maybe</span>
                    </button>
                    <button className={`nav-icon ${route === "blackhole" ? "active" : ""}`} title={t("nav.blackhole")} onClick={() => setRoute("blackhole")}>
                        <span className="ms">deblur</span>
                    </button>
                    <button className={`nav-icon ${route === "souzu-debug" ? "active" : ""}`} title="Souzu Debug" onClick={() => setRoute("souzu-debug")}>
                        <span className="ms">science</span>
                    </button>
                    <button className={`nav-icon ${route === "autorun" ? "active" : ""}`} title={t("nav.autorun")} onClick={() => setRoute("autorun")}>
                        <span className="ms">autoplay</span>
                    </button>
                    <button className={`nav-icon ${route === "diagnostics" ? "active" : ""}`} title={t("nav.diagnostics")} onClick={() => setRoute("diagnostics")}>
                        <span className="ms">article</span>
                    </button>
                    <button className={`nav-icon ${route === "about" ? "active" : ""}`} title={t("nav.about")} onClick={() => setRoute("about")}>
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
                        className="app-main"
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
                        {route === "about" && <AboutPage/>}
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
                            <span className="badge">{t("status.remaining", {count: remain})}</span>
                            <span className="badge">{t("status.stage", {stage: stage})}</span>
                            <span className="badge">{t("status.coin", {coin: coin})}</span>
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
