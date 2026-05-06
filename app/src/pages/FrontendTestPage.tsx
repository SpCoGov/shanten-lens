import React from "react";
import {pushToast, type ToastKind} from "../lib/toast";
import {openMsgBoxWindow} from "../lib/msgbox";
import {ws} from "../lib/ws";

type PacketStats = {
    total?: number;
    by_type?: Record<string, number>;
    by_direction?: Record<string, number>;
    by_method?: Record<string, number>;
    by_type_method?: Record<string, Record<string, number>>;
};

type MitmFlowSummary = {
    id: number;
    peer_key: string;
    client: string;
    server: string;
    request: string;
    messages: number;
    is_preferred: boolean;
    is_last: boolean;
    packet_stats?: PacketStats;
    exclusive_packets?: Array<{packet_type: string; method: string; count: number}>;
};

function formatCounts(counts?: Record<string, number>) {
    const entries = Object.entries(counts || {})
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
    if (!entries.length) return "无";
    return entries.map(([key, value]) => `${key}: ${value}`).join(" / ");
}

function formatTopMethods(stats?: PacketStats, limit = 8) {
    const entries = Object.entries(stats?.by_type_method || {})
        .flatMap(([packetType, methods]) =>
            Object.entries(methods || {}).map(([method, count]) => ({
                label: `${packetType} ${method}`,
                count,
            })),
        )
        .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label))
        .slice(0, limit);
    if (!entries.length) return "无";
    return entries.map((item) => `${item.label}: ${item.count}`).join("\n");
}

function formatExclusivePackets(flow: MitmFlowSummary, limit = 8) {
    const entries = (flow.exclusive_packets || [])
        .slice()
        .sort((a, b) => b.count - a.count || `${a.packet_type} ${a.method}`.localeCompare(`${b.packet_type} ${b.method}`))
        .slice(0, limit);
    if (!entries.length) return "无";
    return entries.map((item) => `${item.packet_type} ${item.method}: ${item.count}`).join("\n");
}

function sendToast(kind: ToastKind) {
    const textMap: Record<ToastKind, string> = {
        info: "这是一条信息提示",
        success: "操作成功，状态已更新",
        error: "操作失败，请检查输入或重试",
    };
    pushToast(textMap[kind], kind, 2200);
}

export default function FrontendTestPage() {
    const [msgboxMessage, setMsgboxMessage] = React.useState("这是一个前端测试用消息框。");
    const [msgboxTitle, setMsgboxTitle] = React.useState("前端测试");

    const [mitmFlowStatus, setMitmFlowStatus] = React.useState("");
    const [mitmFlows, setMitmFlows] = React.useState<MitmFlowSummary[]>([]);
    const [shouldThrowRenderError, setShouldThrowRenderError] = React.useState(false);

    if (shouldThrowRenderError) {
        throw new Error("Manual frontend render error test");
    }

    React.useEffect(() => {
        ws.connect();
        const offPacket = ws.onPacket((pkt) => {
            if (pkt.type !== "mitm_dump_flows_result") return;
            if (pkt.data?.ok) {
                const flows = Array.isArray(pkt.data.flows) ? pkt.data.flows as MitmFlowSummary[] : [];
                const count = Number(pkt.data.count ?? pkt.data.flows?.length ?? 0);
                setMitmFlows(flows);
                setMitmFlowStatus(`已打印 ${count} 个 flow 到后端日志`);
            } else {
                setMitmFlows([]);
                setMitmFlowStatus(`打印失败：${String(pkt.data?.reason || "unknown")}`);
            }
        });
        return offPacket;
    }, []);

    const openConfirmMsgBox = async () => {
        await openMsgBoxWindow({
            id: `frontend-test-${Date.now()}`,
            title: msgboxTitle.trim() || "前端测试",
            message: msgboxMessage.trim() || "这是一个前端测试用消息框。",
            okText: "确认",
            cancelText: "取消",
        });
    };

    const openAlertMsgBox = async () => {
        await openMsgBoxWindow({
            id: `frontend-test-alert-${Date.now()}`,
            title: msgboxTitle.trim() || "前端测试",
            message: msgboxMessage.trim() || "这是一个前端测试用消息框。",
            okText: "知道了",
        });
    };

    const dumpMitmFlows = () => {
        setMitmFlowStatus("正在请求后端打印 flow...");
        setMitmFlows([]);
        ws.send({type: "mitm_dump_flows", data: {}} as any);
    };

    return (
        <div className="diag-wrap">
            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>MITM Flow 测试</h3>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center"}}>
                    <button className="btn" onClick={dumpMitmFlows}>打印当前所有 Flow</button>
                    {mitmFlowStatus && <span className="hint">{mitmFlowStatus}</span>}
                </div>
                {mitmFlows.length > 0 && (
                    <div style={{display: "grid", gap: 8}}>
                        {mitmFlows.map((flow) => {
                            const stats = flow.packet_stats || {};
                            const flags = [
                                flow.is_preferred ? "preferred" : "",
                                flow.is_last ? "last" : "",
                            ].filter(Boolean).join(" / ");
                            return (
                                <div
                                    key={`${flow.id}-${flow.peer_key}`}
                                    style={{
                                        display: "grid",
                                        gap: 4,
                                        padding: "8px 10px",
                                        border: "1px solid var(--border)",
                                        borderRadius: 6,
                                    }}
                                >
                                    <div style={{display: "flex", gap: 8, flexWrap: "wrap", alignItems: "baseline"}}>
                                        <strong>{flow.peer_key || `flow-${flow.id}`}</strong>
                                        {flags && <span className="hint">{flags}</span>}
                                        <span className="hint">frames: {flow.messages}</span>
                                        <span className="hint">packets: {stats.total || 0}</span>
                                    </div>
                                    <div className="hint">{flow.client} {"->"} {flow.server}</div>
                                    <div className="hint">类型统计：{formatCounts(stats.by_type)}</div>
                                    <div className="hint">方向统计：{formatCounts(stats.by_direction)}</div>
                                    <pre
                                        className="hint"
                                        style={{margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere"}}
                                    >
                                        {`方法统计：\n${formatTopMethods(stats)}`}
                                    </pre>
                                    <pre
                                        className="hint"
                                        style={{margin: 0, whiteSpace: "pre-wrap", overflowWrap: "anywhere"}}
                                    >
                                        {`独有封包：\n${formatExclusivePackets(flow)}`}
                                    </pre>
                                </div>
                            );
                        })}
                    </div>
                )}
            </section>

            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>Toast 测试</h3>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                    <button className="btn" onClick={() => sendToast("info")}>触发 Info Toast</button>
                    <button className="btn" onClick={() => sendToast("success")}>触发 Success Toast</button>
                    <button className="btn" onClick={() => sendToast("error")}>触发 Error Toast</button>
                    <button
                        className="btn"
                        onClick={() => pushToast(`随机提示 @ ${new Date().toLocaleTimeString()}`, "info", 2800)}
                    >
                        触发带时间戳 Toast
                    </button>
                </div>
            </section>

            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>MsgBox 测试</h3>
                <div style={{display: "grid", gap: 8, maxWidth: 720}}>
                    <label style={{display: "grid", gap: 6}}>
                        <span>标题</span>
                        <input
                            className="form-input"
                            value={msgboxTitle}
                            onChange={(e) => setMsgboxTitle(e.target.value)}
                        />
                    </label>
                    <label style={{display: "grid", gap: 6}}>
                        <span>内容</span>
                        <textarea
                            className="form-input"
                            rows={4}
                            value={msgboxMessage}
                            onChange={(e) => setMsgboxMessage(e.target.value)}
                        />
                    </label>
                </div>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                    <button className="btn" onClick={openConfirmMsgBox}>打开确认框（OK/Cancel）</button>
                    <button className="btn" onClick={openAlertMsgBox}>打开提示框（仅 OK）</button>
                </div>
            </section>

            <section className="mj-panel card" style={{display: "grid", gap: 12}}>
                <h3 style={{margin: 0}}>Error Boundary 测试</h3>
                <div style={{display: "flex", gap: 10, flexWrap: "wrap"}}>
                    <button className="btn" onClick={() => setShouldThrowRenderError(true)}>
                        手动抛出渲染错误
                    </button>
                    <button
                        className="btn"
                        onClick={() => {
                            void Promise.reject(new Error("Manual frontend async error test"));
                        }}
                    >
                        手动抛出异步错误
                    </button>
                </div>
            </section>
        </div>
    );
}
