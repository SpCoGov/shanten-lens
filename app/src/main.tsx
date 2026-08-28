import React from "react";
import ReactDOM from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import App from "./App";
import { ensureI18nReady } from "./lib/i18n";
import {AppErrorBoundary} from "./components/AppErrorBoundary";
import {t} from "i18next";

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
    await ensureI18nReady();
    await reportStartupProgress("i18n", t("startup.loading_language"), 0.82, t("startup.loading_language_detail"), 2, false);
    await reportStartupProgress("render", t("startup.rendering"), 0.92, t("startup.rendering_detail"), 1, false);

    renderApp(<App />);
}

bootstrap().catch((error) => {
    renderApp(null);
    queueMicrotask(() => {
        appErrorBoundaryRef.current?.showStartupError(error);
    });
});
