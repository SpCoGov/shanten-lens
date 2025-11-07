import React from "react";
import "./styles/theme.css";
import "./App.css";
import {listen} from "@tauri-apps/api/event";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import AutoRunnerPage from "./pages/AutoRunnerPage";
import FusePage from "./pages/FusePage";
import AboutPage from "./pages/AboutPage";
import {ws} from "./lib/ws";
import {type LogLevel, useLogStore} from "./lib/logStore";
import TileGrid from "./components/TileGrid";
import WallStats from "./components/WallStats";
import ReplacementPanel from "./components/ReplacementPanel";
import AdvisorPanel, {type PlanData} from "./components/AdvisorPanel";
import AmuletBar from "./components/AmuletBar";
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

type Route = "home" | "fuse" | "autorun" | "settings" | "diagnostics" | "about";

const OUTER_PADDING = 16;
const SIDEBAR_WIDTH = 320;
const MAIN_GAP = 12;

const appWindow = getCurrentWindow();

function Topbar() {
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
                <span className="title">向听镜</span>
            </div>

            <div className="win" data-tauri-drag-region="false">
                <button className="win-btn" data-tauri-drag-region="false" title="最小化" onClick={onMin}>
                    <span className="ms">remove</span>
                </button>
                <button className="win-btn" data-tauri-drag-region="false" title="最大化/还原" onClick={onTgl}>
                    <span className="ms">rectangle</span>
                </button>
                <button className="win-btn close" data-tauri-drag-region="false" title="关闭" onClick={onClose}>
                    <span className="ms">close</span>
                </button>
            </div>
        </header>
    );
}

export default function App() {
    const {toast, visible: toastVisible} = useGlobalToast();
    const [route, setRoute] = React.useState<Route>("home");
    const [connected, setConnected] = React.useState(false);

    const [cells, setCells] = React.useState<Cell[]>([]);
    const [stage, setStage] = React.useState<number>(0);
    const [coin, setCoin] = React.useState<number>(0);
    const [ended, setEnded] = React.useState<boolean>(false);
    const [remain, setRemain] = React.useState<number>(0);
    const [hasGame, setHasGame] = React.useState<boolean>(false);

    const [wallStatsTiles, setWallStatsTiles] = React.useState<string[]>([]);

    const [replacementTiles, setReplacementTiles] = React.useState<string[]>([]);
    const [switchUsedCount, setSwitchUsedCount] = React.useState<number>(0);

    const [deckMap, setDeckMap] = React.useState<Map<number, string>>(new Map());

    const [planSuuAnkou, setPlanSuuAnkou] = React.useState<PlanData | null>(null);
    const [planChiitoi, setPlanChiitoi] = React.useState<PlanData | null>(null);

    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);
    const [goods, setGoods] = React.useState<GoodsItem[]>([]);
    const [candidates, setCandidates] = React.useState<CandidateEffectRef[]>([]);

    type ThemeMode = "light" | "dark";
    const THEME_KEY = "sl-theme";

    function applyTheme(t: ThemeMode) {
        const root = document.documentElement;
        if (t === "dark") {
            root.setAttribute("data-theme", "dark");
        } else {
            root.removeAttribute("data-theme");
        }
    }

    const [theme, setTheme] = React.useState<ThemeMode>(() => {
        const saved = localStorage.getItem(THEME_KEY) as ThemeMode | null;
        if (saved === "light" || saved === "dark") return saved;
        const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches;
        return prefersDark ? "dark" : "light";
    });

    React.useEffect(() => {
        applyTheme(theme);
        localStorage.setItem(THEME_KEY, theme);
    }, [theme]);

    const toggleTheme = () => setTheme((prev) => (prev === "dark" ? "light" : "dark"));

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

        // 初次绑定
        bindAll(document);

        // 监听后续新增（路由切换/弹窗）
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

    /* ===== WS & 事件桥接 ===== */
    React.useEffect(() => {
        ws.connect();
        setConnected(ws.connected);
        installWsToastBridge(ws);
        const offOpen = ws.onOpen(() => setConnected(true));
        const offClose = ws.onClose(() => setConnected(false));

        const offPkt = ws.onPacket((pkt: WsEnvelope) => {
            if (pkt.type === "update_gamestate") {
                const d = pkt.data as GameStateData;
                const deck = toDeckMap(d.deck_map);
                setDeckMap(deck);
                const list = buildCells(deck, d.locked_tiles ?? [], d.wall_tiles ?? [], 36);
                setCells(list);
                setStage(d.stage ?? 0);
                setCoin(d.coin ?? 0);
                setEnded(!!d.ended);
                setRemain(d.desktop_remain ?? 0);
                setHasGame(d.stage !== undefined && d.ended !== undefined && d.stage >= 0);

                const repl = Array.isArray(d.replacement_tiles)
                    ? d.replacement_tiles.map((id) => deck.get(id) ?? "5m")
                    : [];
                const used = Array.isArray((d as any).switch_used_tiles) ? (d as any).switch_used_tiles.length : 0;
                setReplacementTiles(repl);
                setSwitchUsedCount(used);

                const wallList = Array.isArray(d.wall_tiles) ? d.wall_tiles.map((id) => deck.get(id) ?? "5m") : [];
                setWallStatsTiles(wallList);

                setPlanSuuAnkou(null);
                setPlanChiitoi(null);

                setAmulets(Array.isArray(d.effect_list) ? d.effect_list : []);
                setGoods(d.goods ?? []);
                setCandidates(d.candidate_effect_list ?? []);
            } else if (pkt.type === "discard_recommendation" && pkt.data) {
                const arr = (Array.isArray(pkt.data) ? pkt.data : []) as Array<{ yaku: string; data: PlanData }>;
                for (const item of arr) {
                    if (!item || !item.yaku) continue;
                    if (item.yaku === "chiitoi") setPlanChiitoi(item.data ?? null);
                    else if (item.yaku === "suuannkou") setPlanSuuAnkou(item.data ?? null);
                }
            } else if (pkt.type === "autorun_status" && pkt.data) {
                setAutoStatus(pkt.data as AutoRunnerStatus);
            }
        });

        const addLog = useLogStore.getState().addLog;
        let unsubs: Array<() => void> = [];
        (async () => {
            const sub = async (event: string, level: LogLevel = "INFO") => {
                const un = await listen<string>(event, (e) => {
                    const payload = e.payload;
                    addLog(level, `${event}: ${payload}`);
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

    const Dot = ({ok}: { ok: boolean }) => (
        <span className={`dot ${ok ? "ok" : "down"}`} aria-label={ok ? "connected" : "disconnected"}/>
    );

    return (
        <div className="app">
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>

            <Topbar/>

            <div className="shell">
                <aside className="sidebar">
                    <button className={`nav-icon ${route === "home" ? "active" : ""}`} title="主页" onClick={() => setRoute("home")}>
                        <span className="ms">home</span>
                    </button>
                    <button className={`nav-icon ${route === "fuse" ? "active" : ""}`} title="熔断" onClick={() => setRoute("fuse")}>
                        <span className="ms">gpp_maybe</span>
                    </button>
                    <button className={`nav-icon ${route === "autorun" ? "active" : ""}`} title="自动化" onClick={() => setRoute("autorun")}>
                        <span className="ms">autoplay</span>
                    </button>
                    <button className={`nav-icon ${route === "settings" ? "active" : ""}`} title="设置" onClick={() => setRoute("settings")}>
                        <span className="ms">settings</span>
                    </button>
                    <button className={`nav-icon ${route === "diagnostics" ? "active" : ""}`} title="日志" onClick={() => setRoute("diagnostics")}>
                        <span className="ms">article</span>
                    </button>
                    <button className={`nav-icon ${route === "about" ? "active" : ""}`} title="关于" onClick={() => setRoute("about")}>
                        <span className="ms">help</span>
                    </button>
                    <button
                        className="nav-icon"
                        title={`切换主题（当前：${theme === "dark" ? "深色" : "浅色"}）`}
                        onClick={toggleTheme}
                    >
                        <span className="ms">{theme === "dark" ? "dark_mode" : "light_mode"}</span>
                    </button>
                </aside>

                <main className="main-pane">
                    <div className="app-main" style={{padding: `${OUTER_PADDING}px`, boxSizing: "border-box"}}>
                        {route === "home" && (
                            <div
                                className="home-grid"
                                style={{display: "flex", alignItems: "stretch", gap: MAIN_GAP, minHeight: "100%"}}
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
                                        <div className="panel-title">护身符</div>
                                        <AmuletBar items={amulets} scale={0.55}/>
                                    </div>

                                    {(stage === 4 || stage === 5) && (
                                        <div className="panel">
                                            <div className="panel-title">商品</div>
                                            <GoodsBar items={goods} scale={0.85}/>
                                        </div>
                                    )}

                                    {[1, 5, 7].includes(stage) && (
                                        <div className="panel">
                                            <div className="panel-title">候选护身符</div>
                                            <CandidateBar candidates={candidates} scale={0.55}/>
                                        </div>
                                    )}

                                    {(stage === 2 || stage === 3) && <TileGrid cells={cells}/>}

                                    {stage === 2 && replacementTiles.length > 0 && (
                                        <ReplacementPanel replacementTiles={replacementTiles} usedCount={switchUsedCount}/>
                                    )}
                                </div>

                                {(stage === 2 || stage === 3) && (
                                    <div className="panel" style={{flex: "0 0 auto", width: "auto", marginRight: 0}}>
                                        <WallStats wallTiles={wallStatsTiles}/>
                                    </div>
                                )}
                            </div>
                        )}

                        {route === "fuse" && <FusePage/>}
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
              {connected ? "后端：已连接" : "后端：未连接"}
          </span>
                </div>

                <div className="sb-right">
                    {hasGame ? (
                        <>
                            <span className="badge">剩余：{remain}</span>
                            <span className="badge">阶段：{stage}</span>
                            <span className="badge">⭐ {coin}</span>
                            <span className={`badge ${ended ? "down" : "ok"}`}>{ended ? "已结束" : "进行中"}</span>
                        </>
                    ) : (
                        <span className="badge down">未找到游戏</span>
                    )}
                </div>
            </footer>
        </div>
    );
}