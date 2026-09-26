import React from "react";
import {emit, listen} from "@tauri-apps/api/event";
import {getCurrentWindow} from "@tauri-apps/api/window";
import PacketViewer, {type JsonViewerContent} from "../components/PacketViewer";
import {initializeBackend} from "../lib/ipc";
import {useGlobalToast} from "../lib/toast";
import {useTranslation} from "react-i18next";

export default function PacketViewerWindow() {
    const [packet, setPacket] = React.useState<JsonViewerContent | null>(null);
    const {t} = useTranslation();
    const {toast, visible} = useGlobalToast();

    React.useEffect(() => {
        void initializeBackend();
        let active = true;
        let unlisten: (() => void) | undefined;
        void listen<JsonViewerContent>("packet-viewer-packet", (event) => setPacket(event.payload))
            .then((off) => {
                if (!active) return off();
                unlisten = off;
                return emit("packet-viewer-ready", getCurrentWindow().label);
            });
        return () => {
            active = false;
            unlisten?.();
        };
    }, []);

    React.useEffect(() => {
        if (packet) void getCurrentWindow().setTitle(t("kind" in packet ? "diagnostics.log_fields_editor_title" : "diagnostics.packet_viewer_title"));
    }, [packet, t]);

    return (
        <>
            <div className={`toast ${visible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>
            {packet
                ? <PacketViewer packet={packet} standalone onClose={() => void getCurrentWindow().close()}/>
                : <div style={{height: "100%", background: "var(--panel-bg)"}}/>}
        </>
    );
}
