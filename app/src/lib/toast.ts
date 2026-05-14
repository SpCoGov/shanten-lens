export type ToastKind = "info" | "success" | "error";

type ToastPayload = { msg: string; kind?: ToastKind; duration?: number };
import i18n from "./i18n";

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
            const values = {...(d.msg_values ?? {})};
            if (values.ssl === "on" || values.ssl === "off") {
                values.ssl = i18n.t(`autorun.email_error.ssl_${values.ssl}`);
            }
            if (values.detail) {
                values.detail = i18n.t("autorun.email_error.detail", {detail: values.detail});
            }
            if (values.reason_key) {
                const reasonValues = {...(values.reason_values ?? {})};
                if (reasonValues.ssl === "on" || reasonValues.ssl === "off") {
                    reasonValues.ssl = i18n.t(`autorun.email_error.ssl_${reasonValues.ssl}`);
                }
                if (reasonValues.detail) {
                    reasonValues.detail = i18n.t("autorun.email_error.detail", {detail: reasonValues.detail});
                }
                values.reason = String(i18n.t(String(values.reason_key), reasonValues));
            }
            const msg = d.msg_key ? String(i18n.t(String(d.msg_key), values)) : String(d.msg ?? "");
            pushToast(msg, (d.kind ?? "info") as ToastKind, Number(d.duration ?? 2200));
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
