import React from "react";
import {t} from "i18next";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";

type CapturedError = {
    title: string;
    message: string;
    stack?: string;
    componentStack?: string;
    source: "render" | "runtime" | "promise" | "startup" | "backend";
    timestamp: string;
};

type AppErrorBoundaryState = {
    error: CapturedError | null;
};

function stringifyReason(reason: unknown) {
    if (reason instanceof Error) return reason.message;
    if (typeof reason === "string") return reason;
    try {
        return JSON.stringify(reason, null, 2);
    } catch {
        return String(reason);
    }
}

function toCapturedError(
    error: unknown,
    source: CapturedError["source"],
    title: string,
    componentStack?: string
): CapturedError {
    const message = stringifyReason(error) || t("app.error_boundary.unknown_error");
    const stack = error instanceof Error ? error.stack : undefined;
    return {
        title,
        message,
        stack,
        componentStack,
        source,
        timestamp: new Date().toLocaleString(),
    };
}

function AppErrorDialog({error, onReload}: { error: CapturedError; onReload: () => void }) {
    const details = [
        error.stack ? `${t("app.error_boundary.stack")}:\n${error.stack}` : null,
        error.componentStack ? `${t("app.error_boundary.component_stack")}:\n${error.componentStack}` : null,
    ].filter(Boolean).join("\n\n");

    return (
        <div
            className="app-error-overlay"
            role="dialog"
            aria-modal="true"
            aria-labelledby="app-error-title"
        >
            <div className="app-error-shell">
                <div className="app-error-mark" aria-hidden="true">
                    <span className="ms">error</span>
                </div>

                <div className="app-error-copy">
                    <h2 id="app-error-title">{error.title}</h2>
                    <p>{t(error.source === "backend" ? "app.error_boundary.backend_message" : "app.error_boundary.message")}</p>
                </div>

                <div className="app-error-meta" aria-label={t("app.error_boundary.metadata")}>
                    <span>{t("app.error_boundary.source")}: {t(`app.error_boundary.sources.${error.source}`, {defaultValue: error.source})}</span>
                    <span>{t("app.error_boundary.time")}: {error.timestamp}</span>
                </div>

                <div className="app-error-message">
                    <h3>{t("app.error_boundary.message_label")}</h3>
                    <pre>{error.message}</pre>
                </div>

                {details ? (
                    <div className="app-error-details">
                        <h3>{t("app.error_boundary.details_label")}</h3>
                        <pre>{details}</pre>
                    </div>
                ) : null}

                <button className="app-error-action" onClick={onReload}>
                    <span className="ms" aria-hidden="true">refresh</span>
                    {t(error.source === "backend" ? "app.error_boundary.restart" : "app.error_boundary.reload")}
                </button>
            </div>
        </div>
    );
}

export class AppErrorBoundary extends React.Component<React.PropsWithChildren, AppErrorBoundaryState> {
    private stopBackendListener?: () => void;
    state: AppErrorBoundaryState = {
        error: null,
    };

    static getDerivedStateFromError(error: unknown): AppErrorBoundaryState {
        return {
            error: toCapturedError(error, "render", t("app.error_boundary.render_title")),
        };
    }

    componentDidMount() {
        window.addEventListener("error", this.handleRuntimeError);
        window.addEventListener("unhandledrejection", this.handleUnhandledRejection);
        if (isTauri()) {
            let active = true;
            const listener = listen<string>("backend:panic", ({payload}) => {
                if (active) this.showBackendError(payload);
            });
            this.stopBackendListener = () => {
                active = false;
                void listener.then((unlisten) => unlisten()).catch(console.error);
            };
            void listener.then(() => invoke<string | null>("get_backend_panic")).then((report) => {
                if (active && report) this.showBackendError(report);
            }).catch(console.error);
        }
    }

    componentWillUnmount() {
        this.stopBackendListener?.();
        window.removeEventListener("error", this.handleRuntimeError);
        window.removeEventListener("unhandledrejection", this.handleUnhandledRejection);
    }

    componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
        this.setState({
            error: toCapturedError(
                error,
                "render",
                t("app.error_boundary.render_title"),
                errorInfo.componentStack ?? undefined
            ),
        });
    }

    showStartupError = (error: unknown) => {
        this.setState({
            error: toCapturedError(error, "startup", t("app.error_boundary.startup_title")),
        });
    };

    private showBackendError = (report: string) => {
        this.setState({error: toCapturedError(report, "backend", t("app.error_boundary.backend_title"))});
    };

    private handleRuntimeError = (event: ErrorEvent) => {
        this.setState((state) => state.error?.source === "backend" ? null : ({
            error: toCapturedError(event.error ?? event.message, "runtime", t("app.error_boundary.runtime_title")),
        }));
    };

    private handleUnhandledRejection = (event: PromiseRejectionEvent) => {
        this.setState((state) => state.error?.source === "backend" ? null : ({
            error: toCapturedError(event.reason, "promise", t("app.error_boundary.promise_title")),
        }));
    };

    private reload = () => {
        if (this.state.error?.source === "backend") {
            void invoke("restart_app").catch(console.error);
            return;
        }
        window.location.reload();
    };

    render() {
        if (this.state.error) {
            return <AppErrorDialog error={this.state.error} onReload={this.reload}/>;
        }

        return this.props.children;
    }
}
