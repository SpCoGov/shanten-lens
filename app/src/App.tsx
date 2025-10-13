import React from "react";
import {listen} from "@tauri-apps/api/event";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import {ws} from "./lib/ws";
import {useLogStore, type LogLevel} from "./lib/logStore";
import TileGrid from "./components/TileGrid";
import "./App.css";

type Route = "home" | "settings" | "diagnostics";

export default function App() {
    const [route, setRoute] = React.useState<Route>("home");
    const [connected, setConnected] = React.useState(false);

    const demoIds = [1,
        2,
        3,
        4,
        5,
        6,
        7,
        8,
        9,
        10,
        11,
        12,
        13,
        14,
        15,
        16,
        17,
        18,
        19,
        20,
        21,
        22,
        23,
        24,
        25,
        26,
        27,
        28,
        29,
        30,
        31,
        32,
        33,
        34,
        35,
        36,];
    const repeatedIds = React.useMemo(() => Array.from({length: 36}, (_, i) => demoIds[i % demoIds.length]), []);
    const idToTile: Record<number, string> = {
        1:  "0m",
        2:  "1m",
        3:  "2m",
        4:  "3m",
        5:  "4m",
        6:  "5m",
        7:  "6m",
        8:  "7m",
        9:  "8m",
        10: "9m",
        11: "0p",
        12: "1p",
        13: "2p",
        14: "3p",
        15: "4p",
        16: "5p",
        17: "6p",
        18: "7p",
        19: "8p",
        20: "9p",
        21: "0s",
        22: "1s",
        23: "2s",
        24: "3s",
        25: "4s",
        26: "5s",
        27: "6s",
        28: "7s",
        29: "8s",
        30: "9s",
        31: "1z",
        32: "2z",
        33: "3z",
        34: "4z",
        35: "5z",
        36: "6z",
    };
    const tiles = repeatedIds.map((id) => idToTile[id] ?? "bd");

    React.useEffect(() => {
        ws.connect();
        setConnected(ws.connected);
        const off = ws.on(() => setConnected(true));
        const iv = setInterval(() => setConnected(ws.connected), 1500);
        return () => {
            off();
            clearInterval(iv);
        };
    }, []);

    React.useEffect(() => {
        const addLog = useLogStore.getState().addLog;
        let unsubs: Array<() => void> = [];
        (async () => {
            const sub = async (event: string, level: LogLevel = "INFO") => {
                const un = await listen<string>(event, (e) => {
                    const payload = typeof e.payload === "string" ? e.payload : JSON.stringify(e.payload);
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
            unsubs.forEach((u) => u());
            unsubs = [];
        };
    }, []);

    const Dot = ({ok}: { ok: boolean }) => (
        <span className={`dot ${ok ? "ok" : "down"}`} aria-label={ok ? "connected" : "disconnected"}/>
    );


    return (
        <div className="app">
            <header className="app-header">
                <div className="brand">Shanten Lens</div>
                <nav className="nav">
                    <button className={`nav-btn ${route === "home" ? "active" : ""}`} onClick={() => setRoute("home")}>主页</button>
                    <button className={`nav-btn ${route === "settings" ? "active" : ""}`} onClick={() => setRoute("settings")}>设置</button>
                    <button className={`nav-btn ${route === "diagnostics" ? "active" : ""}`} onClick={() => setRoute("diagnostics")}>诊断</button>
                </nav>
                <div className="status">
                    <Dot ok={connected}/>
                    <span>{connected ? "已连接后端" : "未连接"}</span>
                </div>
            </header>

            <main className="app-main">
                {route === "home" && (
                    <TileGrid tiles={tiles}/>
                )}
                {route === "settings" && <SettingsPage/>}
                {route === "diagnostics" && <DiagnosticsPage/>}
            </main>
        </div>
    );
}