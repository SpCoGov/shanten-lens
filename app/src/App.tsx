import React from "react";
import {listen} from "@tauri-apps/api/event";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import AutoRunnerPage from "./pages/AutoRunnerPage";
import FusePage from "./pages/FusePage";
import AboutPage from "./pages/AboutPage";
import {ws} from "./lib/ws";
import {useLogStore, type LogLevel} from "./lib/logStore";
import TileGrid from "./components/TileGrid";
import WallStats from "./components/WallStats";
import ReplacementPanel from "./components/ReplacementPanel";
import AdvisorPanel, {type ChiitoiData} from "./components/AdvisorPanel";
import AmuletBar from "./components/AmuletBar";
import {type EffectItem} from "./lib/gamestate";
import "./App.css";

import {
    type GameStateData,
    type WsEnvelope,
    toDeckMap,
    buildCells,
    type Cell,
} from "./lib/gamestate";

type Route = "home" | "fuse" | "autorun" | "settings" | "diagnostics" | "about";

const OUTER_PADDING = 16;
const SIDEBAR_WIDTH = 320;
const MAIN_GAP = 12;

export default function App() {
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

    const [speedData, setSpeedData] = React.useState<ChiitoiData | null>(null);
    const [countData, setCountData] = React.useState<ChiitoiData | null>(null);
    const [amulets, setAmulets] = React.useState<EffectItem[]>([]);

    React.useEffect(() => {
        ws.connect();
        setConnected(ws.connected);
        const offOpen = ws.onOpen(() => setConnected(true));
        const offClose = ws.onClose(() => setConnected(false));

        const offPkt = ws.onPacket((pkt: WsEnvelope) => {
            if (pkt.type === "update_gamestate") {
                const d = pkt.data as GameStateData;
                const deck = toDeckMap(d.deck_map);

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

                setSpeedData(null);
                setCountData(null);

                setAmulets(Array.isArray(d.effect_list) ? d.effect_list : []);
            } else if (pkt.type === "chiitoi_recommendation" && pkt.data) {
                const data = pkt.data as ChiitoiData;
                if (data.policy === "speed") setSpeedData(data);
                if (data.policy === "count") setCountData(data);
            }
        });

        const addLog = useLogStore.getState().addLog;
        let unsubs: Array<() => void> = [];
        (async () => {
            const sub = async (event: string, level: LogLevel = "INFO") => {
                const un = await listen<string>(event, (e) => {
                    const payload =
                        typeof e.payload === "string" ? e.payload : JSON.stringify(e.payload);
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
                }}
            >
                {route === "home" && (
                    <div
                        className="full-bleed"
                        style={{
                            display: "flex",
                            alignItems: "stretch",
                            gap: 12,
                            height: "100%",
                        }}
                    >
                        <div style={{width: 320, flex: "0 0 auto"}}>
                            <AdvisorPanel speed={speedData} count={countData}/>
                        </div>

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

                            <TileGrid cells={cells}/>

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
                                    gap: 12,
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

                        <div style={{flex: "0 0 auto", width: "auto", marginRight: 0,}}><WallStats wallTiles={wallStatsTiles}/></div>
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