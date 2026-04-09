import React from "react";
import "../styles/theme.css";
import {ws} from "../lib/ws";
import {useLogStore} from "../lib/logStore";
import styles from "./DiagnosticsPage.module.css";
import {useTranslation} from "react-i18next";
import type {FrameItem, LogItem} from "../lib/logStore";

const ESTIMATED_ROW_HEIGHT = 28;
const OVERSCAN_ROWS = 8;

function upperBound(values: number[], target: number) {
    let lo = 0;
    let hi = values.length;
    while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (values[mid] <= target) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

function VirtualLogList<T>({
                               id,
                               items,
                               emptyText,
                               className,
                               onContainerReady,
                               renderRow,
                           }: {
    id: string;
    items: T[];
    emptyText: string;
    className: string;
    onContainerReady?: (el: HTMLDivElement | null) => void;
    renderRow: (item: T, index: number) => React.ReactNode;
}) {
    const containerRef = React.useRef<HTMLDivElement | null>(null);
    const [viewportHeight, setViewportHeight] = React.useState(0);
    const [scrollTop, setScrollTop] = React.useState(0);

    React.useEffect(() => {
        const el = containerRef.current;
        if (!el) return;
        onContainerReady?.(el);
        setViewportHeight(el.clientHeight);

        const ro = new ResizeObserver(() => {
            setViewportHeight(el.clientHeight);
        });
        ro.observe(el);
        return () => {
            ro.disconnect();
            onContainerReady?.(null);
        };
    }, [onContainerReady]);

    const layout = React.useMemo(() => {
        const offsets = new Array<number>(items.length);
        let top = 0;
        for (let i = 0; i < items.length; i += 1) {
            offsets[i] = top;
            top += ESTIMATED_ROW_HEIGHT;
        }
        return {offsets, totalHeight: top};
    }, [items]);

    const startIndex = React.useMemo(() => {
        const idx = upperBound(layout.offsets, Math.max(0, scrollTop)) - 1;
        return Math.max(0, idx - OVERSCAN_ROWS);
    }, [layout.offsets, scrollTop]);

    const endIndex = React.useMemo(() => {
        const bottom = scrollTop + viewportHeight;
        const idx = upperBound(layout.offsets, bottom);
        return Math.min(items.length, idx + OVERSCAN_ROWS);
    }, [items.length, layout.offsets, scrollTop, viewportHeight]);

    const visible = items.slice(startIndex, endIndex);

    return (
        <div
            id={id}
            ref={containerRef}
            className={className}
            onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        >
            {items.length === 0 && <div className="empty">{emptyText}</div>}
            {items.length > 0 && (
                <div className={styles.virtualSpacer} style={{height: layout.totalHeight}}>
                    {visible.map((item, offset) => {
                        const index = startIndex + offset;
                        return (
                            <div
                                key={index}
                                className={styles.virtualRow}
                                style={{top: layout.offsets[index] ?? 0}}
                            >
                                {renderRow(item, index)}
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}

export default function DiagnosticsPage() {
    const {t} = useTranslation();
    const logs = useLogStore((s) => s.logs);
    const frames = useLogStore((s) => s.frames);
    const [tail, setTail] = React.useState(true);
    const [conn, setConn] = React.useState(ws.connected);
    const framesRef = React.useRef<HTMLDivElement | null>(null);
    const logsRef = React.useRef<HTMLDivElement | null>(null);

    React.useEffect(() => {
        ws.connect();
        const iv = setInterval(() => setConn(ws.connected), 1000);
        return () => clearInterval(iv);
    }, []);

    React.useEffect(() => {
        if (!tail) return;
        const el1 = framesRef.current;
        const el2 = logsRef.current;
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
                <VirtualLogList<FrameItem>
                    id="ws-frames"
                    items={frames}
                    emptyText={t("diagnostics.empty_frames")}
                    className={`log ${styles.noAnchor}`}
                    onContainerReady={(el) => {
                        framesRef.current = el;
                    }}
                    renderRow={(f, i) => (
                        <div key={i} className={`${styles.line} ${f.dir === "in" ? styles.info : styles.out}`}>
                            <span className={styles.ts}>[{f.ts}]</span>{" "}
                            <span className={styles.lv}>[{f.dir.toUpperCase()}]</span>{" "}
                            <span className="selectable">{f.raw}</span>
                        </div>
                    )}
                />
            </section>

            <section className="mj-panel card">
                <h3 style={{marginTop: 0}}>{t("diagnostics.section_logs_title")}</h3>
                <VirtualLogList<LogItem>
                    id="diag-logpanel"
                    items={logs}
                    emptyText={t("diagnostics.empty_logs")}
                    className={`log ${styles.noAnchor}`}
                    onContainerReady={(el) => {
                        logsRef.current = el;
                    }}
                    renderRow={(l, i) => {
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
                    }}
                />
            </section>
        </div>
    );
}
