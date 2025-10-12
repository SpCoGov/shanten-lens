import React from "react";
import { listen } from "@tauri-apps/api/event";
import SettingsPage from "./pages/SettingsPage";
import DiagnosticsPage from "./pages/DiagnosticsPage";
import { ws } from "./lib/ws";
import { useLogStore, type LogLevel } from "./lib/logStore";
import "./App.css"; // 全局样式一次性引入

type Route = "home" | "settings" | "diagnostics";

export default function App() {
    const [route, setRoute] = React.useState<Route>("home");
    const [connected, setConnected] = React.useState(false);

    // 建立 WS 连接并跟踪连接状态
    React.useEffect(() => {
        ws.connect();
        setConnected(ws.connected);
        const off = ws.on(() => setConnected(true));
        const iv = setInterval(() => setConnected(ws.connected), 1500);
        return () => { off(); clearInterval(iv); };
    }, []);

    // 全局订阅 Tauri 后端事件 → 写入日志仓库
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
        return () => { unsubs.forEach((u) => u()); unsubs = []; };
    }, []);

    const Dot = ({ ok }: { ok: boolean }) => (
        <span className={`dot ${ok ? "ok" : "down"}`} aria-label={ok ? "connected" : "disconnected"} />
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
                    <Dot ok={connected} />
                    <span>{connected ? "已连接后端" : "未连接"}</span>
                </div>
            </header>

            <main className="app-main">
                {route === "home" && (
                    <section className="card hero">
                        <h2>欢迎 👋</h2>
                        <p>点击“设置”配置应用；需要查看日志与 WS 封包时进入“诊断”。</p>
                        <div className="hero-status">
                            <Dot ok={connected} />
                            <span className={`badge ${connected ? "ok" : "down"}`}>
                {connected ? "WebSocket 正常" : "等待后端…"}
              </span>
                        </div>
                    </section>
                )}
                {route === "settings" && <SettingsPage />}
                {route === "diagnostics" && <DiagnosticsPage />}
            </main>
        </div>
    );
}