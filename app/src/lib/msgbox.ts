import {WebviewWindow, getAllWebviewWindows} from "@tauri-apps/api/webviewWindow";

export type MsgBoxPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string;
    values?: Record<string, unknown>;
};

const opening = new Set<string>();

function logPayload(p: MsgBoxPayload) {
    console.log("[openMsgBoxWindow] id:", p.id,
        " title:", p.title,
        " message:", p.message,
        " okText:", p.okText,
        " cancelText:", p.cancelText,
        " values:", p.values);
}

function encodePayload(obj: unknown): string {
    try {
        const json = JSON.stringify(obj);
        return encodeURIComponent(btoa(unescape(encodeURIComponent(json))));
    } catch {
        return "";
    }
}

export async function openMsgBoxWindow(payload: MsgBoxPayload) {
    const label = `msgbox-${payload.id}`;
    if (opening.has(label)) return;
    opening.add(label);
    const safety = setTimeout(() => opening.delete(label), 5000);

    logPayload(payload);

    try {
        const existing = (await getAllWebviewWindows()).find(w => w.label === label);
        if (existing) {
            await existing.show();
            await existing.setFocus();
            opening.delete(label);
            clearTimeout(safety);
            return;
        }

        const p = encodePayload(payload);
        const url = `msgbox.html?id=${encodeURIComponent(payload.id)}${p ? `&p=${p}` : ""}`;

        const win = new WebviewWindow(label, {
            url,
            title: "Message",
            width: 520,
            height: 220,
            resizable: false,
            decorations: false,
            center: true,
            alwaysOnTop: true,
            visible: true,
        });

        win.once("tauri://created", () => {
            opening.delete(label);
            clearTimeout(safety);
            console.log("[msgbox] created:", label);
        });

        win.once("tauri://error", (e) => {
            opening.delete(label);
            clearTimeout(safety);
            console.error("[msgbox] create error:", e);
        });
    } catch (e) {
        opening.delete(label);
        clearTimeout(safety);
        throw e;
    }
}