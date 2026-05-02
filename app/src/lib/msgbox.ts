import {WebviewWindow, getAllWebviewWindows} from "@tauri-apps/api/webviewWindow";
import i18n from "./i18n";

export type MsgBoxPayload = {
    id: string;
    title?: string;
    message: string;
    okText?: string;
    cancelText?: string;
    values?: Record<string, unknown>;
};

const opening = new Set<string>();
const MIN_WIDTH = 320;
const MAX_WIDTH = 520;
const MIN_HEIGHT = 170;
const MAX_HEIGHT = 560;

function clamp(n: number, min: number, max: number) {
    return Math.max(min, Math.min(max, n));
}

function visualUnits(line: string) {
    let units = 0;
    for (const ch of line) {
        const code = ch.codePointAt(0) ?? 0;
        // CJK / 全角字符按 2 个单位计算，ASCII 按 1 个单位
        const wide = code > 0xFF || /[^\u0000-\u00ff]/.test(ch);
        units += wide ? 2 : 1;
    }
    return units;
}

function displayTextForEstimate(textOrKey: string, values?: Record<string, unknown>) {
    const translated =
        i18n.isInitialized && i18n.exists(textOrKey)
            ? String(i18n.t(textOrKey, values))
            : textOrKey;

    return translated
        .replace(/<br\s*\/?>/gi, "\n")
        .replace(/<\/(?:p|div|li|h[1-6])>/gi, "\n")
        .replace(/<[^>]+>/g, "");
}

function estimateMsgBoxSize(payload: MsgBoxPayload) {
    const text = displayTextForEstimate(String(payload.message ?? ""), payload.values);
    const lines = text.split(/\r?\n/);
    const maxLineUnits = Math.max(8, ...lines.map((line) => visualUnits(line)));
    const hasCancel = !!payload.cancelText;

    // 近似按字符宽度估算：中文 + 混排场景下体验更稳定
    const contentUnits = clamp(maxLineUnits, 14, 48);
    const width = clamp(220 + contentUnits * 7.6, MIN_WIDTH, MAX_WIDTH);

    // 估算换行后的总“可见行数”
    const charsPerLine = Math.max(12, Math.floor((width - 36) / 7.6));
    const wrappedRows = lines.reduce((sum, line) => {
        const units = Math.max(1, visualUnits(line));
        return sum + Math.max(1, Math.ceil(units / charsPerLine));
    }, 0);

    // header + body padding + footer + lineHeight * rows + native inset
    const footerHeight = hasCancel ? 62 : 56;
    const estimatedHeight = 48 + 28 + footerHeight + wrappedRows * 30 + 29;
    const height = clamp(estimatedHeight, MIN_HEIGHT, MAX_HEIGHT);

    return {
        width: Math.round(width),
        height: Math.round(height),
    };
}

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
        const size = estimateMsgBoxSize(payload);

        const win = new WebviewWindow(label, {
            url,
            title: "Message",
            width: size.width,
            height: size.height,
            minWidth: MIN_WIDTH,
            minHeight: MIN_HEIGHT,
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
