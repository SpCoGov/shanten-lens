import React from "react";
import {getCurrentWindow} from "@tauri-apps/api/window";
import {ws} from "../lib/ws";
import styles from "./MsgBoxWindow.module.css";
import {Trans, useTranslation} from "react-i18next";
import "../lib/i18n";
import {listen} from "@tauri-apps/api/event";
import {setAppLanguage} from "../lib/i18n";

type InitPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string;
    values?: Record<string, unknown>;
};

export default function MsgBoxWindow() {
    const {t} = useTranslation();
    const appWindow = getCurrentWindow();
    const [data, setData] = React.useState<InitPayload | null>(null);

    const idRef = React.useRef<string>("");

    function decodePayloadParam(): InitPayload | null {
        try {
            const url = new URL(location.href);
            idRef.current = idRef.current || url.searchParams.get("id") || "";
            const p = url.searchParams.get("p");
            if (!p) return null;
            const json = decodeURIComponent(escape(atob(p)));
            const obj = JSON.parse(json);
            if (obj && typeof obj === "object" && String(obj.id || "") === idRef.current) {
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
        let un = () => {
        };
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
                ws.send({type: "msgbox_ready", data: {id: idRef.current} as any});
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
                    cancelText: p.cancelText || "common.cancel",
                    values: p.values || {},
                });
            }
        });

        ws.connect();
        if (ws.connected && idRef.current) {
            ws.send({type: "msgbox_ready", data: {id: idRef.current} as any});
        }

        return () => {
            offOpen();
            offPkt();
        };
    }, []);

    const close = async () => {
        try {
            await appWindow.close();
        } catch {
        }
    };

    const reply = async (ok: boolean) => {
        const id = idRef.current;
        if (!id) return close();
        ws.send({type: "msgbox_result", data: {id, ok} as any});
        await close();
    };

    const values = data?.values || {};

    return (
        <div className={styles.wrap}>
            <header className={styles.header} data-tauri-drag-region>
                <div className={styles.title} data-tauri-drag-region>
                    <Trans i18nKey={data?.title || "msgbox.defaultTitle"} values={values}/>
                </div>
                <div className={styles.actions}>
                    <button className={styles.iconBtn} onClick={close} title={t("window.close") as string}>
                        <span className="ms">close</span>
                    </button>
                </div>
            </header>

            <main className={styles.main}>
                <div className={styles.message} title={t(data?.message || "msgbox.defaultMessage", values) as string}>
                    <Trans
                        i18nKey={data?.message || "msgbox.defaultMessage"}
                        values={values}
                        components={{b: <b/>, i: <i/>, code: <code/>, br: <br/>}}
                    />
                </div>

                <div className={styles.btns}>
                    <button className={`btn ghost ${styles.btn}`} onClick={() => reply(false)}>
                        <Trans i18nKey={data?.cancelText || "common.cancel"}/>
                    </button>
                    <button className={`btn ${styles.btn}`} onClick={() => reply(true)}>
                        <Trans i18nKey={data?.okText || "common.ok"}/>
                    </button>
                </div>
            </main>
        </div>
    );
}