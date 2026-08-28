import React from "react";
import {
    createJSONEditor,
    Mode,
    toJSONContent,
    type Content,
    type OnChangeStatus,
} from "vanilla-jsoneditor";
import "vanilla-jsoneditor/themes/jse-theme-dark.css";
import * as backendIpc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import {useTranslation} from "react-i18next";
import styles from "./PacketViewer.module.css";

export type PacketViewerPacket = {
    direction: "inbound" | "outbound" | string;
    type: string;
    method: string;
    id?: number | null;
    data: unknown;
    ts_ms?: number;
};

type Editor = ReturnType<typeof createJSONEditor>;

export default function PacketViewer({packet, onClose, standalone = false}: {packet: PacketViewerPacket; onClose: () => void; standalone?: boolean}) {
    const {t} = useTranslation();
    const containerRef = React.useRef<HTMLDivElement | null>(null);
    const editorRef = React.useRef<Editor | null>(null);
    const [contentError, setContentError] = React.useState(false);
    const [replaying, setReplaying] = React.useState(false);
    const [connected, setConnected] = React.useState(false);
    const [dark, setDark] = React.useState(() => isDarkTheme());

    React.useEffect(() => {
        void backendIpc.getSnapshot().then(() => setConnected(true)).catch(() => setConnected(false));
    }, []);

    React.useEffect(() => {
        const observer = new MutationObserver(() => setDark(isDarkTheme()));
        observer.observe(document.documentElement, {attributes: true, attributeFilter: ["data-theme"]});
        const media = window.matchMedia("(prefers-color-scheme: dark)");
        const onMedia = () => setDark(isDarkTheme());
        media.addEventListener("change", onMedia);
        return () => {
            observer.disconnect();
            media.removeEventListener("change", onMedia);
        };
    }, []);

    React.useEffect(() => {
        if (!containerRef.current) return;
        const editor = createJSONEditor({
            target: containerRef.current,
            props: {
                content: {json: packet.data as any},
                mode: Mode.tree,
                mainMenuBar: true,
                navigationBar: true,
                statusBar: true,
                truncateTextSize: 1_024,
                ariaLabel: t("diagnostics.packet_editor_label"),
                onChange: (_content: Content, _previous: Content, status: OnChangeStatus) => setContentError(Boolean(status.contentErrors)),
            },
        });
        editorRef.current = editor;
        return () => {
            editorRef.current = null;
            void editor.destroy();
        };
    }, [packet, t]);

    React.useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") onClose();
        };
        window.addEventListener("keydown", onKeyDown);
        return () => window.removeEventListener("keydown", onKeyDown);
    }, [onClose]);

    const replay = async () => {
        if (contentError || !editorRef.current) {
            pushToast(t("diagnostics.packet_json_invalid"), "error", 2600);
            return;
        }
        try {
            const payload = toJSONContent(editorRef.current.get()).json;
            if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
                pushToast(t("diagnostics.packet_json_object_required"), "error", 2600);
                return;
            }
            setReplaying(true);
            const result = await backendIpc.replayPacket(packet.method, payload as Record<string, backendIpc.JsonValue>);
            if (result.ok) pushToast(t("diagnostics.replay_success", {id: result.msg_id}), "success", 2600);
            else pushToast(t("diagnostics.replay_failed", {reason: result.reason ?? "unknown"}), "error", 3600);
        } catch {
            pushToast(t("diagnostics.packet_json_invalid"), "error", 2600);
        } finally {
            setReplaying(false);
        }
    };

    const replayable = packet.direction === "outbound";
    const dialog = (
            <section className={`${styles.dialog} ${standalone ? styles.standalone : ""}`} role="dialog" aria-modal={!standalone} aria-label={t("diagnostics.packet_viewer_title")}>
                <header className={styles.header}>
                    <div>
                        <h3>{t("diagnostics.packet_viewer_title")}</h3>
                        <div className={styles.meta}>
                            <span><b>{t("diagnostics.packet_method")}</b>{packet.method}</span>
                            <span><b>{t("diagnostics.packet_id")}</b>{packet.id == null ? "-" : packet.id}</span>
                            <span><b>{t("diagnostics.packet_direction")}</b>{t(`diagnostics.direction_${packet.direction}`)}</span>
                        </div>
                    </div>
                    <button type="button" className={styles.close} aria-label={t("diagnostics.close_detail")} onClick={onClose}>×</button>
                </header>
                <div className={`${styles.editor} ${dark ? "jse-theme-dark" : ""}`} ref={containerRef}/>
                <footer className={styles.footer}>
                    <span className={contentError ? styles.error : undefined}>
                        {contentError ? t("diagnostics.packet_json_invalid") : t("diagnostics.packet_editor_hint")}
                    </span>
                    <div>
                        <button type="button" className={styles.secondary} onClick={onClose}>{t("diagnostics.close_detail")}</button>
                        {replayable && (
                            <button type="button" className={styles.primary} disabled={contentError || replaying || !connected} onClick={replay}>
                                {replaying ? t("diagnostics.replaying") : t("diagnostics.replay_packet")}
                            </button>
                        )}
                    </div>
                </footer>
            </section>
    );
    if (standalone) return dialog;
    return (
        <div className={styles.backdrop} role="presentation" onMouseDown={(event) => {
            if (event.target === event.currentTarget) onClose();
        }}>
            {dialog}
        </div>
    );
}

function isDarkTheme() {
    const theme = document.documentElement.getAttribute("data-theme");
    return Boolean(theme) || window.matchMedia("(prefers-color-scheme: dark)").matches;
}
