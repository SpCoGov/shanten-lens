import React from "react";
import "../styles/theme.css";
import {ws} from "../lib/ws";
import {useLogStore} from "../lib/logStore";
import styles from "./DiagnosticsPage.module.css";
import {useTranslation} from "react-i18next";

export default function DiagnosticsPage() {
    const {t} = useTranslation();
    const logs = useLogStore((s) => s.logs);
    const frames = useLogStore((s) => s.frames);
    const [tail, setTail] = React.useState(true);
    const [conn, setConn] = React.useState(ws.connected);

    React.useEffect(() => {
        ws.connect();
        const iv = setInterval(() => setConn(ws.connected), 1000);
        return () => clearInterval(iv);
    }, []);

    React.useEffect(() => {
        if (!tail) return;
        const el1 = document.getElementById("ws-frames");
        const el2 = document.getElementById("diag-logpanel");
        if (el1) el1.scrollTop = el1.scrollHeight;
        if (el2) el2.scrollTop = el2.scrollHeight;
    }, [frames, logs, tail]);

    return (
        <div className="diag-wrap">
            <section className="card diag-top">
                <div className="diag-status">
                    <span className={`dot ${conn ? "ok" : "down"}`}/>
                    <b>{t("diagnostics.ws_status_label")}</b>：{conn ? t("diagnostics.ws_connected") : t("diagnostics.ws_disconnected")}
                </div>
                <label className="tail">
                    <input type="checkbox" checked={tail} onChange={(e) => setTail(e.target.checked)}/>
                    {t("diagnostics.auto_scroll")}
                </label>
            </section>

            <section className="mj-panel card">
                <h3 style={{marginTop: 0}}>{t("diagnostics.section_frames_title")}</h3>
                <div id="ws-frames" className="log ${styles.noAnchor}">
                    {frames.map((f, i) => (
                        <div key={i} className={`${styles.line} ${f.dir === "in" ? styles.info : styles.out}`}>
                            <span className={styles.ts}>[{f.ts}]</span>{" "}
                            <span className={styles.lv}>[{f.dir.toUpperCase()}]</span>{" "}
                            <span className="selectable">{f.raw}</span>
                        </div>
                    ))}
                    {frames.length === 0 && <div className="empty">{t("diagnostics.empty_frames")}</div>}
                </div>
            </section>

            <section className="mj-panel card">
                <h3 style={{marginTop: 0}}>{t("diagnostics.section_logs_title")}</h3>
                <div id="diag-logpanel" className="log ${styles.noAnchor}">
                    {logs.map((l, i) => {
                        const cls =
                            l.level === "ERROR" || l.level === "STDERR" ? styles.err :
                                l.level === "WARN" ? styles.warn :
                                    l.level === "STDOUT" ? styles.out : styles.info;
                        return (
                            <div key={i} className={`${styles.line} ${cls}`}>
                                <span className={styles.ts}>[{l.ts}]</span>{" "}
                                <span className={styles.lv}>[{l.level}]</span>{" "}
                                <span>{l.msg}</span>
                            </div>
                        );
                    })}
                    {logs.length === 0 && <div className="empty">{t("diagnostics.empty_logs")}</div>}
                </div>
            </section>
        </div>
    );
}