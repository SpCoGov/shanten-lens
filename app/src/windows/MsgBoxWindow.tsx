import React from "react";
import { getCurrentWindow, LogicalSize, currentMonitor } from "@tauri-apps/api/window";
import { Trans, useTranslation } from "react-i18next";

import * as backendIpc from "../lib/ipc";
import { setAppLanguage } from "../lib/i18n";
import { safeListen } from "../lib/tauriRuntime";
import "../lib/i18n";
import styles from "./MsgBoxWindow.module.css";

type InitPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string | null;
    values?: Record<string, unknown>;
};

export default function MsgBoxWindow() {
    const { t } = useTranslation();
    const appWindow = React.useMemo(() => getCurrentWindow(), []);

    const [data, setData] = React.useState<InitPayload | null>(null);
    const idRef = React.useRef<string>("");

    const rootRef = React.useRef<HTMLDivElement | null>(null);
    const messageRef = React.useRef<HTMLDivElement | null>(null);
    const centeredOnceRef = React.useRef(false);

    function decodePayloadParam(): InitPayload | null {
        try {
            const url = new URL(location.href);
            idRef.current = idRef.current || url.searchParams.get("id") || "";
            const p = url.searchParams.get("p");
            if (!p) return null;
            const json = decodeURIComponent(escape(atob(p)));
            const obj = JSON.parse(json);
            if (obj && typeof obj === "object" && String((obj as any).id || "") === idRef.current) {
                return obj as InitPayload;
            }
            return null;
        } catch {
            return null;
        }
    }

    if (!idRef.current) {
        const url = new URL(location.href);
        idRef.current = url.searchParams.get("id") || "";
    }

    React.useEffect(() => {
        let active = true;
        let un = () => {};
        (async () => {
            un = await safeListen<{ lng: string }>("i18n:set-language", (e) => {
                setAppLanguage(e.payload.lng);
            });
            if (!active) un();
        })();
        return () => { active = false; un(); };
    }, []);

    React.useEffect(() => {
        const initial = decodePayloadParam();
        setData(
            initial ?? {
                id: idRef.current || "",
                title: "msgbox.defaultTitle",
                message: "msgbox.defaultMessage",
                okText: "common.ok",
                cancelText: "common.cancel",
                values: {},
            }
        );
    }, []);

    React.useEffect(() => {
        if (!rootRef.current) return;

        const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v));

        let cancelled = false;
        let running = false;
        let pending = false;
        let raf: number | null = null;

        const schedule = () => {
            if (cancelled) return;
            if (running) { pending = true; return; }
            if (raf !== null) cancelAnimationFrame(raf);
            raf = requestAnimationFrame(() => { raf = null; void measureAndResize(); });
        };

        const measureAndResize = async () => {
            if (cancelled || !rootRef.current) return;
            running = true;
            try {
                const scale = await appWindow.scaleFactor();
                const cur = (await appWindow.innerSize()).toLogical(scale);
                const mon = await currentMonitor();
                if (cancelled || !rootRef.current) return;
                const monitorSize = mon?.size.toLogical(mon.scaleFactor);
                const maxW = Math.max(320, Math.min(520, Math.floor((monitorSize?.width ?? 1920) * 0.5)));
                const maxH = Math.max(170, Math.min(560, Math.floor((monitorSize?.height ?? 1080) * 0.68)));
                // The root fills the viewport: its dimensions cannot measure natural content.
                // Keep the initial content-based width and let the body wrap within it.
                const targetW = clamp(Math.round(cur.width), 320, maxW);
                const header = rootRef.current.querySelector<HTMLElement>(`.${styles.header}`);
                const main = rootRef.current.querySelector<HTMLElement>(`.${styles.main}`);
                const footer = rootRef.current.querySelector<HTMLElement>(`.${styles.btns}`);
                const mainStyle = main ? getComputedStyle(main) : null;
                const padding = (parseFloat(mainStyle?.paddingTop || "0") || 0)
                    + (parseFloat(mainStyle?.paddingBottom || "0") || 0);
                const naturalHeight = (header?.offsetHeight ?? 0) + padding
                    + (messageRef.current?.scrollHeight ?? 0) + (footer?.offsetHeight ?? 0);
                const targetH = clamp(Math.ceil(naturalHeight), 170, maxH);
                if (Math.abs(cur.width - targetW) >= 2 || Math.abs(cur.height - targetH) >= 2) {
                    await appWindow.setSize(new LogicalSize(targetW, targetH));
                }
                if (!cancelled && !centeredOnceRef.current) {
                    centeredOnceRef.current = true;
                    await appWindow.center();
                }
            } catch {
                // The window may have closed while the native calls were pending.
            } finally {
                running = false;
                if (pending) { pending = false; schedule(); }
            }
        };

        const ro = new ResizeObserver(schedule);
        ro.observe(rootRef.current);
        if (messageRef.current) ro.observe(messageRef.current);
        schedule();
        const timer = window.setTimeout(schedule, 80);

        return () => {
            cancelled = true;
            ro.disconnect();
            if (raf) cancelAnimationFrame(raf);
            if (timer) window.clearTimeout(timer);
        };
    }, [appWindow, data]);

    const close = async () => {
        try {
            await appWindow.close();
        } catch {
            /* no-op */
        }
    };

    const hasCancel = !!data?.cancelText;

    const reply = async (ok: boolean) => {
        const id = idRef.current;
        if (!id) return close();
        await backendIpc.resolveConfirmation(id, ok);
        await close();
    };

    const closeFromChrome = async () => {
        if (!data) return close();
        await reply(!hasCancel);
    };

    const values = data?.values || {};

    return (
        <div className={styles.wrap} ref={rootRef}>
            <header className={styles.header} data-tauri-drag-region>
                <div className={styles.title} data-tauri-drag-region>
                    <Trans i18nKey={data?.title || "msgbox.defaultTitle"} values={values} />
                </div>
                <div className={styles.actions}>
                    <button className={styles.iconBtn} onClick={closeFromChrome} title={t("window.close") as string}>
                        <span className="ms">close</span>
                    </button>
                </div>
            </header>

            <main className={styles.main}>
                <div
                    className={styles.message}
                    ref={messageRef}
                    title={t(data?.message || "msgbox.defaultMessage", values) as string}
                >
                    <Trans
                        i18nKey={data?.message || "msgbox.defaultMessage"}
                        values={values}
                        components={{ b: <b />, i: <i />, code: <code />, br: <br /> }}
                    />
                </div>
            </main>

            <footer className={styles.btns}>
                {hasCancel ? (
                    <button className={`btn ghost ${styles.btn}`} onClick={() => reply(false)}>
                        <Trans i18nKey={data?.cancelText || "common.cancel"} />
                    </button>
                ) : null}
                <button className={`btn ${styles.btn}`} onClick={() => reply(true)}>
                    <Trans i18nKey={data?.okText || "common.ok"} />
                </button>
            </footer>
        </div>
    );
}
