import React from "react";
import {useTranslation} from "react-i18next";
import {pushToast, type ToastKind} from "../lib/toast";
import {openMsgBoxWindow} from "../lib/msgbox";
import * as backendIpc from "../lib/ipc";

type PacketStats = {
    total?: number;
    by_type?: Record<string, number>;
    by_direction?: Record<string, number>;
    by_method?: Record<string, number>;
    by_type_method?: Record<string, Record<string, number>>;
};

type MitmFlowSummary = backendIpc.FlowDumpResult["flows"][number] & {
    request?: string;
    messages?: number;
    is_last?: boolean;
    packet_stats?: PacketStats;
    exclusive_packets?: Array<{packet_type: string; method: string; count: number}>;
};

function formatCounts(counts: Record<string, number> | undefined, empty: string) {
    const entries = Object.entries(counts || {}).sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    return entries.length ? entries.map(([key, value]) => `${key}: ${value}`).join(" / ") : empty;
}

function formatTopMethods(stats: PacketStats | undefined, empty: string, limit = 8) {
    const entries = Object.entries(stats?.by_type_method || {})
        .flatMap(([packetType, methods]) => Object.entries(methods || {}).map(([method, count]) => ({label: `${packetType} ${method}`, count})))
        .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label))
        .slice(0, limit);
    return entries.length ? entries.map((item) => `${item.label}: ${item.count}`).join("\n") : empty;
}

function formatExclusivePackets(flow: MitmFlowSummary, empty: string, limit = 8) {
    const entries = (flow.exclusive_packets || []).slice()
        .sort((a, b) => b.count - a.count || `${a.packet_type} ${a.method}`.localeCompare(`${b.packet_type} ${b.method}`))
        .slice(0, limit);
    return entries.length ? entries.map((item) => `${item.packet_type} ${item.method}: ${item.count}`).join("\n") : empty;
}

export default function FrontendTestPage() {
    const {t} = useTranslation();
    const [msgboxMessage, setMsgboxMessage] = React.useState(() => t("frontend_test.default_message"));
    const [msgboxTitle, setMsgboxTitle] = React.useState(() => t("frontend_test.default_title"));
    const [mitmFlowStatus, setMitmFlowStatus] = React.useState("");
    const [mitmFlows, setMitmFlows] = React.useState<MitmFlowSummary[]>([]);
    const [shouldThrowRenderError, setShouldThrowRenderError] = React.useState(false);
    const empty = t("frontend_test.empty");

    if (shouldThrowRenderError) throw new Error(t("frontend_test.render_error"));

    const sendToast = (kind: ToastKind) => pushToast(t(`frontend_test.toast_${kind}`), kind, 2200);
    const openConfirmMsgBox = async () => openMsgBoxWindow({
        id: `frontend-test-${Date.now()}`,
        title: msgboxTitle.trim() || t("frontend_test.default_title"),
        message: msgboxMessage.trim() || t("frontend_test.default_message"),
        okText: t("common.confirm"),
        cancelText: t("common.cancel"),
    });
    const openAlertMsgBox = async () => openMsgBoxWindow({
        id: `frontend-test-alert-${Date.now()}`,
        title: msgboxTitle.trim() || t("frontend_test.default_title"),
        message: msgboxMessage.trim() || t("frontend_test.default_message"),
        okText: t("frontend_test.acknowledge"),
    });
    const dumpMitmFlows = async () => {
        setMitmFlowStatus(t("frontend_test.requesting_flows"));
        setMitmFlows([]);
        const result = await backendIpc.dumpFlows();
        setMitmFlows(result.flows);
        setMitmFlowStatus(t("frontend_test.flows_dumped", {count: result.count}));
    };

    return <div className="diag-wrap">
        <section className="mj-panel card" style={{display: "grid", gap: 12}}>
            <h3 style={{margin: 0}}>{t("frontend_test.flow_title")}</h3>
            <div style={{display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center"}}>
                <button className="btn" onClick={dumpMitmFlows}>{t("frontend_test.dump_flows")}</button>
                {mitmFlowStatus && <span className="hint">{mitmFlowStatus}</span>}
            </div>
            {mitmFlows.length > 0 && <div style={{display: "grid", gap: 8}}>{mitmFlows.map((flow) => {
                const stats = flow.packet_stats || {};
                const flags = [flow.is_preferred ? t("frontend_test.preferred") : "", flow.is_last ? t("frontend_test.last") : ""].filter(Boolean).join(" / ");
                return <div key={`${flow.id}-${flow.peer_key}`} style={{display: "grid", gap: 4, padding: "8px 10px", border: "1px solid var(--border)", borderRadius: 6}}>
                    <div style={{display: "flex", gap: 8, flexWrap: "wrap", alignItems: "baseline"}}>
                        <strong>{flow.peer_key || `flow-${flow.id}`}</strong>{flags && <span className="hint">{flags}</span>}
                        <span className="hint">{t("frontend_test.frames", {count: flow.messages})}</span>
                        <span className="hint">{t("frontend_test.packets", {count: stats.total || 0})}</span>
                    </div>
                    <div className="hint">{flow.client} {"->"} {flow.server}</div>
                    <div className="hint">{t("frontend_test.type_counts", {value: formatCounts(stats.by_type, empty)})}</div>
                    <div className="hint">{t("frontend_test.direction_counts", {value: formatCounts(stats.by_direction, empty)})}</div>
                    <pre className="hint" style={{margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere"}}>{t("frontend_test.method_counts", {value: formatTopMethods(stats, empty)})}</pre>
                    <pre className="hint" style={{margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere"}}>{t("frontend_test.exclusive_packets", {value: formatExclusivePackets(flow, empty)})}</pre>
                </div>;
            })}</div>}
        </section>

        <section className="mj-panel card" style={{display: "grid", gap: 12}}>
            <h3 style={{margin: 0}}>{t("frontend_test.toast_title")}</h3>
            <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                {(["info", "success", "error"] as ToastKind[]).map((kind) => <button key={kind} className="btn" onClick={() => sendToast(kind)}>{t(`frontend_test.trigger_${kind}_toast`)}</button>)}
                <button className="btn" onClick={() => pushToast(t("frontend_test.timestamp_message", {time: new Date().toLocaleTimeString()}), "info", 2800)}>{t("frontend_test.trigger_timestamp_toast")}</button>
            </div>
        </section>

        <section className="mj-panel card" style={{display: "grid", gap: 12}}>
            <h3 style={{margin: 0}}>{t("frontend_test.msgbox_title")}</h3>
            <div style={{display: "grid", gap: 8, maxWidth: 720}}>
                <label style={{display: "grid", gap: 6}}><span>{t("frontend_test.field_title")}</span><input className="form-input" value={msgboxTitle} onChange={(e) => setMsgboxTitle(e.target.value)}/></label>
                <label style={{display: "grid", gap: 6}}><span>{t("frontend_test.field_content")}</span><textarea className="form-input" rows={4} value={msgboxMessage} onChange={(e) => setMsgboxMessage(e.target.value)}/></label>
            </div>
            <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                <button className="btn" onClick={openConfirmMsgBox}>{t("frontend_test.open_confirm")}</button>
                <button className="btn" onClick={openAlertMsgBox}>{t("frontend_test.open_alert")}</button>
            </div>
        </section>

        <section className="mj-panel card" style={{display: "grid", gap: 12}}>
            <h3 style={{margin: 0}}>{t("frontend_test.error_boundary_title")}</h3>
            <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                <button className="btn" onClick={() => setShouldThrowRenderError(true)}>{t("frontend_test.throw_render")}</button>
                <button className="btn" onClick={() => void Promise.reject(new Error(t("frontend_test.async_error")))}>{t("frontend_test.throw_async")}</button>
            </div>
        </section>
    </div>;
}
