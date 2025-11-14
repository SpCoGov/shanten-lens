import { WebviewWindow, getAllWebviewWindows } from "@tauri-apps/api/webviewWindow";
import { emitTo } from "@tauri-apps/api/event";

export type MsgBoxPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string;
};

const msgboxUrl = import.meta.env.DEV ? `${location.origin}/msgbox.html` : "msgbox.html";

export async function openMsgBoxWindow(payload: MsgBoxPayload) {
    const label = `msgbox-${payload.id}`;

    const existing = (await getAllWebviewWindows()).find(w => w.label === label);
    if (existing) {
        await existing.show();
        await existing.setFocus();
        await emitTo(label, "ui:msgbox:init", payload);
        return;
    }

    const win = new WebviewWindow(label, {
        url: `${msgboxUrl}?id=${encodeURIComponent(payload.id)}`, // 兜底：id 放 query
        title: payload.title || "Message",
        width: 520,
        height: 220,
        resizable: false,
        decorations: false,
        center: true,
        alwaysOnTop: true,
        visible: true
    });

    win.once("tauri://created", async () => {
        // 创建好后把数据发给子窗口
        await emitTo(label, "ui:msgbox:init", payload);
    });

    win.once("tauri://error", (e) => console.error("[msgbox] create error:", e));
}
