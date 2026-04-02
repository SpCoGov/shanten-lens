import React from "react";
import { getCurrentWindow, LogicalSize, currentMonitor } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { Trans, useTranslation } from "react-i18next";

import { ws } from "../lib/ws";
import { setAppLanguage } from "../lib/i18n";
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
    const appWindow = getCurrentWindow();

    const [data, setData] = React.useState<InitPayload | null>(null);
    const idRef = React.useRef<string>("");

    const rootRef = React.useRef<HTMLDivElement | null>(null);
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
        let un = () => {};
        (async () => {
            un = await listen<{ lng: string }>("i18n:set-language", (e) => {
                setAppLanguage(e.payload.lng);
            });
        })();
        return () => un();
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

        const offOpen = ws.onOpen(() => {
            if (idRef.current) {
                ws.send({ type: "msgbox_ready", data: { id: idRef.current } as any });
            }
        });

        const offPkt = ws.onPacket((pkt) => {
            if (pkt.type === "msgbox_init" && pkt.data?.id === idRef.current) {
                const p = pkt.data as InitPayload;
                setData({
                    id: String(p.id),
                    title: p.title || "msgbox.defaultTitle",
                    message: p.message || "msgbox.defaultMessage",
                    okText: p.okText || "common.ok",
                    cancelText: p.cancelText ?? undefined,
                    values: p.values || {},
                });
            }
        });

        ws.connect();
        if (ws.connected && idRef.current) {
            ws.send({ type: "msgbox_ready", data: { id: idRef.current } as any });
        }

        return () => {
            offOpen();
            offPkt();
        };
    }, []);

    React.useEffect(() => {
        if (!rootRef.current) return;

        const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v));

        const measureAndResize = async () => {
            if (!rootRef.current) return;

            // 用内容真实渲染后的几何尺寸
            const rect = rootRef.current.getBoundingClientRect();

            // 给少量余量，避免边界换行导致瞬时滚动条
            let w = Math.ceil(rect.width + 16);
            let h = Math.ceil(rect.height + 16);

            const mon = await currentMonitor();
            const availW = mon?.size?.width ?? 1920;
            const availH = mon?.size?.height ?? 1080;

            const MAX_W = Math.floor(availW * 0.6);
            const MAX_H = Math.floor(availH * 0.8);
            const MIN_W = 420;
            const MIN_H = 200;

            const targetW = clamp(w, MIN_W, MAX_W);
            const targetH = clamp(h, MIN_H, MAX_H);

            try {
                const cur = await appWindow.innerSize();
                const dx = Math.abs(cur.width - targetW);
                const dy = Math.abs(cur.height - targetH);
                if (dx < 2 && dy < 2) return;
            } catch {
                // ignore
            }

            await appWindow.setSize(new LogicalSize(targetW, targetH));
            if (!centeredOnceRef.current) {
                centeredOnceRef.current = true;
                await appWindow.center();
            }
        };

        let raf: number | null = null;
        const ro = new ResizeObserver(() => {
            if (raf) cancelAnimationFrame(raf);
            raf = requestAnimationFrame(measureAndResize);
        });

        ro.observe(rootRef.current);
        void measureAndResize();

        return () => {
            ro.disconnect();
            if (raf) cancelAnimationFrame(raf);
        };
    }, [appWindow]);

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
        ws.send({ type: "msgbox_result", data: { id, ok } as any });
        await close();
    };

    const closeFromChrome = async () => {
        if (!data) return close();
        await reply(hasCancel ? false : true);
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
                <div className={styles.message} title={t(data?.message || "msgbox.defaultMessage", values) as string}>
                    <Trans
                        i18nKey={data?.message || "msgbox.defaultMessage"}
                        values={values}
                        components={{ b: <b />, i: <i />, code: <code />, br: <br /> }}
                    />
                </div>

                <div className={styles.btns}>
                    {hasCancel ? (
                        <button className={`btn ghost ${styles.btn}`} onClick={() => reply(false)}>
                            <Trans i18nKey={data?.cancelText || "common.cancel"} />
                        </button>
                    ) : null}
                    <button className={`btn ${styles.btn}`} onClick={() => reply(true)}>
                        <Trans i18nKey={data?.okText || "common.ok"} />
                    </button>
                </div>
            </main>
        </div>
    );
}
