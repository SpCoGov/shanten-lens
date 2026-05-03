import React from "react";
import ReactDOM from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import App from "./App";
import { ensureI18nReady } from "./lib/i18n";
import {AppErrorBoundary} from "./components/AppErrorBoundary";

const appErrorBoundaryRef = React.createRef<AppErrorBoundary>();
const root = ReactDOM.createRoot(document.getElementById("root")!);

function renderApp(children: React.ReactNode) {
    root.render(
        <React.StrictMode>
            <AppErrorBoundary ref={appErrorBoundaryRef}>
                {children}
            </AppErrorBoundary>
        </React.StrictMode>
    );
}

async function reportStartupProgress(
    phase: string,
    label: string,
    progress: number,
    detail?: string,
    etaSeconds?: number,
    indeterminate?: boolean
) {
    try {
        await invoke("update_startup_progress", {
            phase,
            label,
            detail,
            progress,
            etaSeconds,
            indeterminate,
        });
    } catch {
    }
}

async function bootstrap() {
    await reportStartupProgress("i18n", "正在加载语言与配置", 0.82, "检测系统语言并初始化界面文案", 2, false);
    await ensureI18nReady();
    await reportStartupProgress("render", "正在构建主界面", 0.92, "挂载首屏组件并连接事件", 1, false);

    renderApp(<App />);
}

bootstrap().catch((error) => {
    renderApp(null);
    queueMicrotask(() => {
        appErrorBoundaryRef.current?.showStartupError(error);
    });
});
