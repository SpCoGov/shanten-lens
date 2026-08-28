import React from "react";
import "../styles/theme.css";
import * as backendIpc from "../lib/ipc";
import {useLogStore} from "../lib/logStore";
import styles from "./DiagnosticsPage.module.css";
import {useTranslation} from "react-i18next";
import type {LogItem, LogLevel} from "../lib/logStore";
import type {PacketViewerPacket} from "../components/PacketViewer";
import {openPacketViewerWindow} from "../lib/packetViewerWindow";
import protoMethods from "virtual:liqi-methods";

const LOG_LEVELS: LogLevel[] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE", "STDOUT", "STDERR"];
const DEFAULT_LEVELS: LogLevel[] = ["ERROR", "WARN", "INFO", "DEBUG", "STDOUT", "STDERR"];
const PACKET_CAPACITY = 1_000;

type Tab = "logs" | "packets";
type PacketItem = PacketViewerPacket;

function levelClass(level: LogLevel) {
    if (level === "ERROR" || level === "STDERR") return styles.err;
    if (level === "WARN") return styles.warn;
    if (level === "STDOUT") return styles.out;
    return styles.info;
}

function packetTime(packet: PacketItem) {
    const date = new Date(packet.ts_ms ?? Date.now());
    return `${date.toLocaleTimeString([], {hour12: false})}.${date.getMilliseconds().toString().padStart(3, "0")}`;
}

export default function DiagnosticsPage() {
    const {t} = useTranslation();
    const logs = useLogStore((state) => state.logs);
    const clearLogs = useLogStore((state) => state.clearLogs);
    const [tab, setTab] = React.useState<Tab>("logs");
    const [tail, setTail] = React.useState(true);
    const [conn, setConn] = React.useState(false);
    const [levels, setLevels] = React.useState<Set<LogLevel>>(() => new Set(DEFAULT_LEVELS));
    const [target, setTarget] = React.useState("");
    const [search, setSearch] = React.useState("");
    const [selected, setSelected] = React.useState<LogItem | null>(null);
    const [packets, setPackets] = React.useState<PacketItem[]>([]);
    const [packetDirection, setPacketDirection] = React.useState("");
    const [packetMethod, setPacketMethod] = React.useState("");
    const [packetSearch, setPacketSearch] = React.useState("");
    const [selectedPacket, setSelectedPacket] = React.useState<PacketItem | null>(null);
    const logsRef = React.useRef<HTMLDivElement | null>(null);
    const packetsRef = React.useRef<HTMLDivElement | null>(null);

    React.useEffect(() => {
        void backendIpc.initializeBackend().then(() => setConn(true)).catch(() => setConn(false));
        void backendIpc.getPacketLog().then((snapshot) => setPackets(snapshot.packets.slice(-PACKET_CAPACITY)));
        const off = backendIpc.subscribeBackendEvent("packet_log_event", (packet) => {
            setPackets((current) => [...current.slice(-(PACKET_CAPACITY - 1)), packet as PacketItem]);
        });
        return off;
    }, []);

    const targets = React.useMemo(() => Array.from(new Set(logs.map((log) => log.target))).sort(), [logs]);
    const packetMethodQuery = packetMethod.trim().toLocaleLowerCase();
    const packetMethodSuggestions = React.useMemo(
        () => packetMethodQuery.length < 5
            ? []
            : protoMethods.filter((method) => method.toLocaleLowerCase().includes(packetMethodQuery)).slice(0, 20),
        [packetMethodQuery],
    );
    const filteredLogs = React.useMemo(() => {
        const query = search.trim().toLocaleLowerCase();
        return logs.filter((log) => {
            if (!levels.has(log.level) || (target && log.target !== target)) return false;
            if (!query) return true;
            const fields = log.fields ? JSON.stringify(log.fields) : "";
            return `${log.msg}\n${log.target}\n${fields}`.toLocaleLowerCase().includes(query);
        });
    }, [levels, logs, search, target]);
    const filteredPackets = React.useMemo(() => {
        const fieldQuery = packetSearch.trim().toLocaleLowerCase();
        return packets.filter((packet) => {
            if (packetDirection && packet.direction !== packetDirection) return false;
            if (packetMethodQuery && !packet.method.toLocaleLowerCase().includes(packetMethodQuery)) return false;
            return !fieldQuery || JSON.stringify(packet.data ?? null).toLocaleLowerCase().includes(fieldQuery);
        });
    }, [packetDirection, packetMethodQuery, packetSearch, packets]);
    const selectedLog = selected && filteredLogs.includes(selected) ? selected : null;
    const visibleSelectedPacket = selectedPacket && filteredPackets.includes(selectedPacket) ? selectedPacket : null;

    React.useEffect(() => {
        const list = tab === "logs" ? logsRef.current : packetsRef.current;
        if (!tail || !list) return;
        list.scrollTop = list.scrollHeight;
    }, [filteredLogs.length, filteredPackets.length, tab, tail]);

    const toggleLevel = (level: LogLevel) => {
        setLevels((current) => {
            const next = new Set(current);
            if (next.has(level)) next.delete(level);
            else next.add(level);
            return next;
        });
    };

    return (
        <div className={`diag-wrap ${styles.page}`}>
            <section className="card diag-top">
                <div className="diag-status">
                    <span className={`dot ${conn ? "ok" : "down"}`}/>
                    <b>{t("diagnostics.ipc_status_label")}</b>
                    <span>: {conn ? t("diagnostics.ipc_connected") : t("diagnostics.ipc_disconnected")}</span>
                </div>
                <label className="tail">
                    <input type="checkbox" checked={tail} onChange={(event) => setTail(event.target.checked)}/>
                    {t("diagnostics.auto_scroll")}
                </label>
            </section>

            <section className={`mj-panel card ${styles.viewer}`}>
                <div className={styles.tabs} role="tablist">
                    <button type="button" role="tab" aria-selected={tab === "logs"} className={tab === "logs" ? styles.tabActive : undefined} onClick={() => setTab("logs")}>
                        {t("diagnostics.tab_logs")}
                    </button>
                    <button type="button" role="tab" aria-selected={tab === "packets"} className={tab === "packets" ? styles.tabActive : undefined} onClick={() => setTab("packets")}>
                        {t("diagnostics.tab_packets")}
                    </button>
                </div>

                <div className={styles.logHeader}>
                    <h3>{t(tab === "logs" ? "diagnostics.section_logs_title" : "diagnostics.section_packets_title")}</h3>
                    <span>{tab === "logs" ? t("diagnostics.log_count", {shown: filteredLogs.length, total: logs.length}) : t("diagnostics.packet_count", {shown: filteredPackets.length, total: packets.length})}</span>
                </div>

                {tab === "logs" ? (
                    <>
                        <div className={styles.filters}>
                            <div className={styles.levels} role="group" aria-label={t("diagnostics.level_filter")}>
                                {LOG_LEVELS.map((level) => (
                                    <button key={level} type="button" className={`${levelClass(level)} ${levels.has(level) ? styles.levelActive : ""}`} aria-pressed={levels.has(level)} onClick={() => toggleLevel(level)}>{level}</button>
                                ))}
                            </div>
                            <select className={styles.moduleSelect} value={target} aria-label={t("diagnostics.module_filter")} onChange={(event) => setTarget(event.target.value)}>
                                <option value="">{t("diagnostics.all_modules")}</option>
                                {targets.map((item) => <option key={item} value={item}>{item}</option>)}
                            </select>
                            <input className={styles.logSearch} type="search" value={search} placeholder={t("diagnostics.search_placeholder")} aria-label={t("diagnostics.search_placeholder")} onChange={(event) => setSearch(event.target.value)}/>
                            <button type="button" className={styles.clearButton} onClick={() => { clearLogs(); setSelected(null); }}>{t("diagnostics.clear_logs")}</button>
                        </div>
                        <div className={styles.logWorkspace}>
                            <div id="diag-logpanel" ref={logsRef} className={styles.logList}>
                                {filteredLogs.length === 0 && <div className={styles.empty}>{t("diagnostics.empty_logs")}</div>}
                                {filteredLogs.map((log, index) => (
                                    <button key={`${log.ts_ms}-${log.target}-${index}`} type="button" className={`${styles.logItem} ${levelClass(log.level)} ${selectedLog === log ? styles.selected : ""}`} onClick={() => setSelected(selectedLog === log ? null : log)}>
                                        <span className={styles.ts}>{log.ts}</span><span className={styles.lv}>{log.level}</span><span className={styles.target}>{log.target}</span><span className={styles.message}>{log.msg || "-"}</span>
                                    </button>
                                ))}
                            </div>
                            {selectedLog && (
                                <aside className={styles.detailPanel}>
                                    <DetailHeader title={t("diagnostics.detail_title")} closeLabel={t("diagnostics.close_detail")} onClose={() => setSelected(null)}/>
                                    <div className={styles.detailBody}>
                                        <Detail label={t("diagnostics.detail_time")} value={new Date(selectedLog.ts_ms).toISOString()}/>
                                        <Detail label={t("diagnostics.detail_level")} value={selectedLog.level}/>
                                        <Detail label={t("diagnostics.detail_module")} value={selectedLog.target}/>
                                        {(selectedLog.file || selectedLog.line !== undefined) && (
                                            <Detail label={t("diagnostics.detail_location")} value={`${selectedLog.file ?? "-"}${selectedLog.line !== undefined ? `:${selectedLog.line}` : ""}`}/>
                                        )}
                                        <Detail label={t("diagnostics.detail_message")} value={selectedLog.msg} block/>
                                        {selectedLog.fields && Object.keys(selectedLog.fields).length > 0 && <Detail label={t("diagnostics.detail_fields")} value={JSON.stringify(selectedLog.fields, null, 2)} block/>}
                                    </div>
                                </aside>
                            )}
                        </div>
                    </>
                ) : (
                    <>
                        <div className={styles.filters}>
                            <div className={styles.directionFilter} role="group" aria-label={t("diagnostics.packet_direction")}>
                                <button type="button" className={`${styles.directionAll} ${packetDirection === "" ? styles.directionActive : ""}`} aria-pressed={packetDirection === ""} onClick={() => setPacketDirection("")}>{t("diagnostics.direction_all")}</button>
                                <button type="button" className={`${styles.directionOutbound} ${packetDirection === "outbound" ? styles.directionActive : ""}`} aria-pressed={packetDirection === "outbound"} onClick={() => setPacketDirection("outbound")}>↑ {t("diagnostics.direction_outbound")}</button>
                                <button type="button" className={`${styles.directionInbound} ${packetDirection === "inbound" ? styles.directionActive : ""}`} aria-pressed={packetDirection === "inbound"} onClick={() => setPacketDirection("inbound")}>↓ {t("diagnostics.direction_inbound")}</button>
                            </div>
                            <input className={styles.methodSearch} type="search" value={packetMethod} placeholder={t("diagnostics.packet_method_placeholder")} aria-label={t("diagnostics.packet_method_placeholder")} list={packetMethodQuery.length >= 5 ? "packet-log-method-suggestions" : undefined} autoComplete="off" onChange={(event) => setPacketMethod(event.target.value)}/>
                            <datalist id="packet-log-method-suggestions">
                                {packetMethodSuggestions.map((method) => <option value={method} key={method}/>)}
                            </datalist>
                            <input className={styles.contentSearch} type="search" value={packetSearch} placeholder={t("diagnostics.packet_search_placeholder")} aria-label={t("diagnostics.packet_search_placeholder")} onChange={(event) => setPacketSearch(event.target.value)}/>
                        </div>
                        <div className={styles.logWorkspace}>
                            <div ref={packetsRef} className={styles.logList}>
                                {filteredPackets.length === 0 && <div className={styles.empty}>{t("diagnostics.empty_packets")}</div>}
                                {filteredPackets.map((packet, index) => (
                                    <button key={`${packet.ts_ms ?? 0}-${packet.method}-${packet.id ?? "n"}-${index}`} type="button" className={`${styles.packetItem} ${visibleSelectedPacket === packet ? styles.selected : ""} ${packet === packets[packets.length - 1] ? styles.newPacket : ""}`} onClick={() => setSelectedPacket(visibleSelectedPacket === packet ? null : packet)}>
                                        <span className={styles.ts}>{packetTime(packet)}</span>
                                        <span className={`${styles.direction} ${packet.direction === "outbound" ? styles.outbound : styles.inbound}`}>{packet.direction === "outbound" ? "↑" : "↓"}</span>
                                        <span className={styles.packetType}>{packet.type}</span><span className={styles.message}>{packet.method}</span>
                                    </button>
                                ))}
                            </div>
                            {visibleSelectedPacket && (
                                <aside className={styles.detailPanel}>
                                    <DetailHeader title={t("diagnostics.packet_detail_title")} closeLabel={t("diagnostics.close_detail")} onClose={() => setSelectedPacket(null)}/>
                                    <div className={styles.detailBody}>
                                        <Detail label={t("diagnostics.detail_time")} value={new Date(visibleSelectedPacket.ts_ms ?? Date.now()).toISOString()}/>
                                        <Detail label={t("diagnostics.packet_direction")} value={t(`diagnostics.direction_${visibleSelectedPacket.direction}`)}/>
                                        <Detail label={t("diagnostics.packet_type")} value={visibleSelectedPacket.type}/>
                                        <Detail label={t("diagnostics.packet_method")} value={visibleSelectedPacket.method}/>
                                        <Detail label={t("diagnostics.packet_id")} value={visibleSelectedPacket.id == null ? "-" : String(visibleSelectedPacket.id)}/>
                                        <button type="button" className={styles.viewerButton} onClick={() => void openPacketViewerWindow(visibleSelectedPacket)}>
                                            {t("diagnostics.open_packet_viewer")}
                                        </button>
                                    </div>
                                </aside>
                            )}
                        </div>
                    </>
                )}
            </section>
        </div>
    );
}

function DetailHeader({title, closeLabel, onClose}: {title: string; closeLabel: string; onClose: () => void}) {
    return <div className={styles.detailHeader}><h4>{title}</h4><button type="button" aria-label={closeLabel} onClick={onClose}>×</button></div>;
}

function Detail({label, value, block = false}: {label: string; value: string; block?: boolean}) {
    return <div className={styles.detailField}><span>{label}</span>{block ? <pre>{value}</pre> : <div>{value}</div>}</div>;
}
