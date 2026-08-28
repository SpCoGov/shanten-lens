import {emitTo, listen} from "@tauri-apps/api/event";
import {getAllWebviewWindows, WebviewWindow} from "@tauri-apps/api/webviewWindow";
import type {PacketViewerPacket} from "../components/PacketViewer";
import {pushToast} from "./toast";
import i18n from "./i18n";

const LABEL = "packet-viewer";
let pendingPacket: PacketViewerPacket | null = null;
let ready = false;
let listener: Promise<void> | null = null;

function ensureReadyListener() {
    if (!listener) {
        listener = listen<string>("packet-viewer-ready", (event) => {
            if (event.payload !== LABEL) return;
            ready = true;
            if (pendingPacket) void emitTo(LABEL, "packet-viewer-packet", pendingPacket);
        }).then(() => undefined);
    }
    return listener;
}

export async function openPacketViewerWindow(packet: PacketViewerPacket) {
    pendingPacket = packet;
    try {
        await ensureReadyListener();
        const existing = (await getAllWebviewWindows()).find((window) => window.label === LABEL);
        if (existing) {
            if (ready) await emitTo(LABEL, "packet-viewer-packet", packet);
            await existing.show();
            await existing.setFocus();
            return;
        }

        ready = false;
        const theme = document.documentElement.getAttribute("data-theme") || "light";
        const baseUrl = import.meta.env.DEV ? `${location.origin}/packet-viewer.html` : "packet-viewer.html";
        const window = new WebviewWindow(LABEL, {
            url: `${baseUrl}?theme=${encodeURIComponent(theme)}`,
            title: String(i18n.t("diagnostics.packet_viewer_title")),
            width: 1120,
            height: 780,
            minWidth: 760,
            minHeight: 540,
            resizable: true,
            center: true,
            visible: true,
        });
        window.once("tauri://error", (error) => {
            pushToast(String(error.payload ?? error), "error", 3600);
        });
    } catch (error) {
        pushToast(String(error), "error", 3600);
    }
}
