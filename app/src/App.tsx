import React from "react";
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
import {buildCells, CandidateEffectRef, type Cell, type EffectItem, type GameStateData, type GoodsItem, toDeckMap, type WsEnvelope} from "./lib/gamestate";
import {installWsToastBridge, useGlobalToast} from "./lib/toast";
import {AutoRunnerStatus, setAutoStatus} from "./lib/autoRunnerStore";
import GoodsBar from "./components/GoodsBar";
import CandidateBar from "./components/CandidateBar";
import "./App.css";

type Route = "home" | "fuse" | "autorun" | "settings" | "diagnostics" | "about";

const OUTER_PADDING = 16;
const SIDEBAR_WIDTH = 320;
const MAIN_GAP = 12;

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
                const used = Array.isArray((d as any).switch_used_tiles)
                    ? (d as any).switch_used_tiles.length
                    : 0;
                setReplacementTiles(repl);
                setSwitchUsedCount(used);

                const wallList = Array.isArray(d.wall_tiles)
                    ? d.wall_tiles.map((id) => deck.get(id) ?? "5m")
                    : [];
                setWallStatsTiles(wallList);

                setPlanSuuAnkou(null);
                setPlanChiitoi(null);

                setAmulets(Array.isArray(d.effect_list) ? d.effect_list : []);
                setGoods(d.goods ?? []);
                setCandidates(d.candidate_effect_list ?? []);
            } else if (pkt.type === "discard_recommendation" && pkt.data) {
                const arr = Array.isArray(pkt.data) ? pkt.data as Array<{ yaku: string; data: PlanData }> : [];
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
                    const payload =
                        e.payload;
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
        <span
            className={`dot ${ok ? "ok" : "down"}`}
            aria-label={ok ? "connected" : "disconnected"}
        />
    );

    return (
        <div className="app">
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>
                {toast?.msg}
            </div>

            <header className="app-header">
                <div className="brand">向听镜</div>
                <nav className="nav">
                    <button
                        className={`nav-btn ${route === "home" ? "active" : ""}`}
                        onClick={() => setRoute("home")}
                    >
                        主页
                    </button>
                    <button
                        className={`nav-btn ${route === "fuse" ? "active" : ""}`}
                        onClick={() => setRoute("fuse")}
                    >
                        熔断
                    </button>
                    <button
                        className={`nav-btn ${route === "autorun" ? "active" : ""}`}
                        onClick={() => setRoute("autorun")}
                    >
                        自动化
                    </button>
                    <button
                        className={`nav-btn ${route === "settings" ? "active" : ""}`}
                        onClick={() => setRoute("settings")}
                    >
                        设置
                    </button>
                    <button
                        className={`nav-btn ${route === "diagnostics" ? "active" : ""}`}
                        onClick={() => setRoute("diagnostics")}
                    >
                        诊断
                    </button>
                    <button
                        className={`nav-btn ${route === "about" ? "active" : ""}`}
                        onClick={() => setRoute("about")}
                    >
                        关于
                    </button>
                </nav>
                <div className="status">
                    <Dot ok={connected}/>
                    <span>{connected ? "已连接" : "未连接"}</span>
                </div>
            </header>

            <main
                className="app-main"
                style={{
                    padding: `${OUTER_PADDING}px`,
                    boxSizing: "border-box",
                    height: "calc(100vh - var(--header-height, 56px))",
                    overflowY: "auto",
                }}
            >
                {route === "home" && (
                    <div
                        className="full-bleed"
                        style={{
                            display: "flex",
                            alignItems: "stretch",
                            gap: MAIN_GAP,
                            height: "100%",
                        }}
                    >
                        {(stage === 2 || stage === 3) ? (
                            <div style={{width: SIDEBAR_WIDTH, flex: "0 0 auto"}}>
                                <AdvisorPanel
                                    suuAnkou={planSuuAnkou}
                                    chiitoi={planChiitoi}
                                    resolveFace={(id) => deckMap.get(id) ?? null}
                                />
                            </div>
                        ) : null}

                        <div style={{flex: 1, minWidth: 0, position: "relative"}}>
                            <div
                                style={{
                                    border: "1px solid var(--border, #ddd)",
                                    borderRadius: 12,
                                    background: "#fff",
                                    padding: 8,
                                    marginBottom: 12,
                                }}
                            >
                                <div style={{fontWeight: 600, marginBottom: 6}}>护身符</div>
                                <AmuletBar items={amulets} scale={0.55}/>
                            </div>

                            {(stage === 4 || stage === 5) ? (
                                <div
                                    style={{
                                        border: "1px solid var(--border, #ddd)",
                                        borderRadius: 12,
                                        background: "#fff",
                                        padding: 8,
                                        marginBottom: 12,
                                    }}
                                >
                                    <div style={{fontWeight: 600, marginBottom: 6}}>商品</div>
                                    <GoodsBar items={goods} scale={0.85}/>
                                </div>
                            ) : null}

                            {[1, 5, 7].includes(stage) && (
                                <div
                                    style={{
                                        border: "1px solid var(--border, #ddd)",
                                        borderRadius: 12,
                                        background: "#fff",
                                        padding: 8,
                                        marginBottom: 12,
                                    }}
                                >
                                    <div style={{fontWeight: 600, marginBottom: 6}}>候选护身符</div>
                                    <CandidateBar candidates={candidates} scale={0.55}/>
                                </div>
                            )}

                            {(stage === 2 || stage === 3) ? (
                                <TileGrid cells={cells}/>
                            ) : null}

                            {stage === 2 && replacementTiles.length > 0 && (
                                <ReplacementPanel
                                    replacementTiles={replacementTiles}
                                    usedCount={switchUsedCount}
                                />
                            )}

                            <div
                                style={{
                                    position: "fixed",
                                    left: "50%",
                                    transform: "translateX(-50%)",
                                    bottom: 24,
                                    background: "#fff",
                                    border: "1px solid var(--border)",
                                    borderRadius: 12,
                                    boxShadow: "0 8px 24px rgba(0,0,0,.08)",
                                    padding: "8px 12px",
                                    display: "flex",
                                    alignItems: "center",
                                    gap: MAIN_GAP,
                                    zIndex: 60,
                                }}
                            >
                                {hasGame ? (
                                    <>
                                        <span style={{fontWeight: 600}}>状态</span>
                                        <span className="badge">{`剩余：${remain}`}</span>
                                        <span className="badge">{`阶段：${stage}`}</span>
                                        <span className="badge">{`⭐ ${coin}`}</span>
                                        <span className={`badge ${ended ? "down" : "ok"}`}>{ended ? "已结束" : "进行中"}</span>
                                    </>
                                ) : (
                                    <span className="badge down">未找到游戏</span>
                                )}
                            </div>
                        </div>

                        {(stage === 2 || stage === 3) ? (
                            <div style={{flex: "0 0 auto", width: "auto", marginRight: 0}}>
                                <WallStats wallTiles={wallStatsTiles}/>
                            </div>
                        ) : null}
                    </div>
                )}

                {route === "fuse" && <FusePage/>}
                {route === "autorun" && <AutoRunnerPage/>}
                {route === "settings" && <SettingsPage/>}
                {route === "diagnostics" && <DiagnosticsPage/>}
                {route === "about" && <AboutPage/>}
            </main>
        </div>
    );
}