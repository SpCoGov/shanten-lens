import React from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ws } from "../lib/ws";
import styles from "./MsgBoxWindow.module.css";

type InitPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string;
};

export default function MsgBoxWindow() {
    const appWindow = getCurrentWindow();
    const [data, setData] = React.useState<InitPayload | null>(null);

    React.useEffect(() => {
        const url = new URL(location.href);
        const id = url.searchParams.get("id") || "";
        if (id && !data) setData({ id, message: "", okText: "OK", cancelText: "Cancel" });
        // 接收父窗体的初始化事件
        let un = () => {};
        (async () => {
            un = await listen<InitPayload>("ui:msgbox:init", (e) => setData(e.payload));
        })();
        return () => un();
    }, []);

    const close = async () => {
        try { await appWindow.close(); } catch {}
    };

    const reply = async (ok: boolean) => {
        if (!data?.id) return close();
        ws.send({ type: "msgbox_result", data: { id: data.id, ok } as any });
        await close();
    };

    return (
        <div className={styles.wrap}>
            <header className={styles.header} data-tauri-drag-region>
                <div className={styles.title} data-tauri-drag-region>
                    {data?.title ?? "Message"}
                </div>
                <div className={styles.actions}>
                    <button className={styles.iconBtn} onClick={close} title="Close"><span className="ms">close</span></button>
                </div>
            </header>

            <main className={styles.main}>
                <div className={styles.message} title={data?.message}>{data?.message}</div>
                <div className={styles.btns}>
                    <button className={`btn ghost ${styles.btn}`} onClick={() => reply(false)}>
                        {data?.cancelText ?? "Cancel"}
                    </button>
                    <button className={`btn ${styles.btn}`} onClick={() => reply(true)}>
                        {data?.okText ?? "OK"}
                    </button>
                </div>
            </main>
        </div>
    );
}