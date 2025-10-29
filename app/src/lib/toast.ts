export type ToastKind = "info" | "success" | "error";

type ToastPayload = { msg: string; kind?: ToastKind; duration?: number };

const EVT = "shanten:toast";
let installedWsBridge = false;

export function pushToast(msg: string, kind: ToastKind = "info", duration = 2200) {
    const detail: ToastPayload = { msg, kind, duration };
    window.dispatchEvent(new CustomEvent<ToastPayload>(EVT, { detail }));
}

export function installWsToastBridge(ws: { on: (fn: (pkt: any) => void) => () => void }) {
    if (installedWsBridge) return;
    installedWsBridge = true;
    ws.on((pkt: any) => {
        if (pkt?.type === "ui_toast") {
            const d = pkt.data || {};
            pushToast(String(d.msg ?? ""), (d.kind ?? "info") as ToastKind, Number(d.duration ?? 2200));
        }
    });
}

export function useGlobalToast() {
    const [toast, setToast] = React.useState<{ msg: string; kind: ToastKind; id: number } | null>(null);
    const [visible, setVisible] = React.useState(false);
    const timerRef = React.useRef<number | null>(null);

    React.useEffect(() => {
        const onToast = (e: Event) => {
            const ce = e as CustomEvent<ToastPayload>;
            const { msg, kind = "info", duration = 2200 } = ce.detail || { msg: "" };
            setToast({ msg, kind, id: Date.now() });
            setVisible(true);
            if (timerRef.current) window.clearTimeout(timerRef.current);
            timerRef.current = window.setTimeout(() => setVisible(false), duration);
        };
        window.addEventListener(EVT, onToast as EventListener);
        return () => {
            window.removeEventListener(EVT, onToast as EventListener);
            if (timerRef.current) window.clearTimeout(timerRef.current);
        };
    }, []);

    return { toast, visible };
}

import * as React from "react";