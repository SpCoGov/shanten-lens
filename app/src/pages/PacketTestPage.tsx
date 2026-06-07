import React from "react";
import {ws} from "../lib/ws";
import {pushToast} from "../lib/toast";
import styles from "./PacketTestPage.module.css";

type PacketRecord = {
    seq: number;
    ts: number;
    group_key: string;
    msg_id: number | null;
    packet_type: "Req" | "Res" | "Notify" | string;
    method: string;
    from_client: boolean;
    data: any;
    flow_id?: number | null;
    flow_peer_key?: string | null;
    flow_client?: string | null;
    flow_server?: string | null;
};

type FlowInfo = {
    id?: number | null;
    peer_key?: string | null;
    client?: string | null;
    server?: string | null;
    is_preferred?: boolean;
    is_last?: boolean;
};

type FlowEvent = {
    seq: number;
    ts: number;
    event: "start" | "end" | string;
    flow: FlowInfo;
};

type PacketGroup = {
    key: string;
    method: string;
    msgId: number | null;
    latestTs: number;
    flowId: number | null;
    flowPeerKey: string;
    request: PacketRecord | null;
    response: PacketRecord | null;
    items: PacketRecord[];
};

const STORAGE_COUNT_KEY = "packet-test:display-count";
const STORAGE_BLOCKED_KEY = "packet-test:blocked-methods";
const STORAGE_AUTO_SHOW_KEY = "packet-test:auto-show-new";
const STORAGE_ENABLED_KEY = "packet-test:enabled";

function formatTime(ts: number) {
    const d = new Date(ts);
    const base = [d.getHours(), d.getMinutes(), d.getSeconds()]
        .map((n) => n.toString().padStart(2, "0"))
        .join(":");
    return `${base}.${d.getMilliseconds().toString().padStart(3, "0")}`;
}

function loadDisplayCount() {
    const raw = Number(localStorage.getItem(STORAGE_COUNT_KEY) || "10");
    if (!Number.isFinite(raw)) return 10;
    return Math.min(100, Math.max(1, Math.round(raw)));
}

function loadBlockedMethods() {
    return localStorage.getItem(STORAGE_BLOCKED_KEY) || ".lq.Route.heartbeat";
}

function loadAutoShowNew() {
    return localStorage.getItem(STORAGE_AUTO_SHOW_KEY) !== "0";
}

function loadEnabled() {
    return localStorage.getItem(STORAGE_ENABLED_KEY) === "1";
}

function buildGroups(packets: PacketRecord[]) {
    const map = new Map<string, PacketGroup>();
    for (const packet of packets) {
        const key = packet.group_key || `fallback:${packet.seq}`;
        const existing = map.get(key);
        if (existing) {
            existing.items.push(packet);
            existing.latestTs = Math.max(existing.latestTs, packet.ts);
            if (!existing.method && packet.method) existing.method = packet.method;
            if (existing.msgId == null && packet.msg_id != null) existing.msgId = packet.msg_id;
            if (existing.flowId == null && packet.flow_id != null) existing.flowId = packet.flow_id;
            if (!existing.flowPeerKey && packet.flow_peer_key) existing.flowPeerKey = packet.flow_peer_key;
            if (packet.packet_type === "Req") existing.request = packet;
            if (packet.packet_type === "Res") existing.response = packet;
            continue;
        }
        map.set(key, {
            key,
            method: packet.method,
            msgId: packet.msg_id ?? null,
            latestTs: packet.ts,
            flowId: packet.flow_id ?? null,
            flowPeerKey: packet.flow_peer_key ?? "",
            request: packet.packet_type === "Req" ? packet : null,
            response: packet.packet_type === "Res" ? packet : null,
            items: [packet],
        });
    }

    return Array.from(map.values()).sort((a, b) => b.latestTs - a.latestTs);
}

export default function PacketTestPage() {
    const [packets, setPackets] = React.useState<PacketRecord[]>([]);
    const [flowEvents, setFlowEvents] = React.useState<FlowEvent[]>([]);
    const [selectedFlow, setSelectedFlow] = React.useState("all");
    const [displayCount, setDisplayCount] = React.useState(loadDisplayCount);
    const [enabled, setEnabled] = React.useState(loadEnabled);
    const [blockedMethodsText, setBlockedMethodsText] = React.useState(loadBlockedMethods);
    const [autoShowNew, setAutoShowNew] = React.useState(loadAutoShowNew);
    const [selectedKey, setSelectedKey] = React.useState<string | null>(null);
    const [requestEditorText, setRequestEditorText] = React.useState("{}");

    React.useEffect(() => {
        localStorage.setItem(STORAGE_COUNT_KEY, String(displayCount));
    }, [displayCount]);

    React.useEffect(() => {
        localStorage.setItem(STORAGE_ENABLED_KEY, enabled ? "1" : "0");
    }, [enabled]);

    React.useEffect(() => {
        localStorage.setItem(STORAGE_BLOCKED_KEY, blockedMethodsText);
    }, [blockedMethodsText]);

    React.useEffect(() => {
        localStorage.setItem(STORAGE_AUTO_SHOW_KEY, autoShowNew ? "1" : "0");
    }, [autoShowNew]);

    React.useEffect(() => {
        ws.connect();

        const sendSettings = () => {
            const blockedMethods = blockedMethodsText
                .split(/[\r\n,]+/)
                .map((item) => item.trim())
                .filter(Boolean);
            ws.send({
                type: "packet_monitor_update_settings",
                data: {
                    enabled,
                    blockedMethods,
                },
            } as any);
        };

        const requestSnapshot = () => {
            ws.send({type: "packet_monitor_request_snapshot", data: {}} as any);
        };

        const offOpen = ws.onOpen(() => {
            sendSettings();
            requestSnapshot();
        });
        const offPacket = ws.onPacket((pkt) => {
            if (pkt.type === "packet_monitor_settings" && pkt.data) {
                const nextEnabled = !!pkt.data.enabled;
                const nextBlocked = Array.isArray(pkt.data.blockedMethods) ? pkt.data.blockedMethods : [];
                setEnabled(nextEnabled);
                setBlockedMethodsText(nextBlocked.join("\n"));
                return;
            }
            if (pkt.type === "packet_monitor_snapshot") {
                const next = Array.isArray(pkt.data?.packets) ? pkt.data.packets : [];
                const nextFlowEvents = Array.isArray(pkt.data?.flowEvents) ? pkt.data.flowEvents : [];
                setPackets(next);
                setFlowEvents(nextFlowEvents);
                return;
            }
            if (pkt.type === "packet_monitor_event" && pkt.data) {
                if (autoShowNew && typeof pkt.data.group_key === "string") {
                    setSelectedKey(pkt.data.group_key);
                }
                setPackets((prev) => [...prev, pkt.data as PacketRecord]);
            }
            if (pkt.type === "packet_monitor_flow_event" && pkt.data) {
                setFlowEvents((prev) => [...prev.slice(-199), pkt.data as FlowEvent]);
            }
        });

        sendSettings();
        requestSnapshot();
        return () => {
            offOpen();
            offPacket();
        };
    }, [autoShowNew, blockedMethodsText, enabled]);

    const flowOptions = React.useMemo(() => {
        const map = new Map<string, FlowInfo>();
        for (const packet of packets) {
            if (packet.flow_id == null) continue;
            const key = String(packet.flow_id);
            if (!map.has(key)) {
                map.set(key, {
                    id: packet.flow_id,
                    peer_key: packet.flow_peer_key,
                    client: packet.flow_client,
                    server: packet.flow_server,
                });
            }
        }
        for (const event of flowEvents) {
            const id = event.flow?.id;
            if (id == null) continue;
            map.set(String(id), {...(map.get(String(id)) || {}), ...event.flow});
        }
        return Array.from(map.values()).sort((a, b) => String(a.peer_key || a.id).localeCompare(String(b.peer_key || b.id)));
    }, [flowEvents, packets]);

    const filteredPackets = React.useMemo(() => {
        if (selectedFlow === "all") return packets;
        return packets.filter((packet) => String(packet.flow_id ?? "") === selectedFlow);
    }, [packets, selectedFlow]);

    const filteredFlowEvents = React.useMemo(() => {
        const items = selectedFlow === "all"
            ? flowEvents
            : flowEvents.filter((event) => String(event.flow?.id ?? "") === selectedFlow);
        return items.slice(-20).reverse();
    }, [flowEvents, selectedFlow]);

    const visibleGroups = React.useMemo(() => {
        return buildGroups(filteredPackets)
            .slice(0, displayCount);
    }, [displayCount, filteredPackets]);

    React.useEffect(() => {
        if (!visibleGroups.length) {
            setSelectedKey(null);
            return;
        }
        if (!selectedKey || !visibleGroups.some((group) => group.key === selectedKey)) {
            setSelectedKey(visibleGroups[0].key);
        }
    }, [selectedKey, visibleGroups]);

    const selectedGroup = visibleGroups.find((group) => group.key === selectedKey) ?? null;
    const selectedRequest = selectedGroup?.request ?? null;
    const selectedResponse = selectedGroup?.response ?? null;
    const selectedFallback = selectedGroup?.items[0] ?? null;

    React.useEffect(() => {
        if (!selectedRequest) {
            setRequestEditorText("{}");
            return;
        }
        setRequestEditorText(JSON.stringify(selectedRequest.data, null, 2));
    }, [selectedRequest?.seq]);

    const replaySelected = () => {
        if (!selectedRequest) return;
        let payload: any;
        try {
            payload = JSON.parse(requestEditorText);
        } catch {
            pushToast("发送封包不是合法 JSON，无法重放", "error", 2400);
            return;
        }
        if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
            pushToast("发送封包必须是 JSON 对象", "error", 2400);
            return;
        }
        ws.send({
            type: "packet_monitor_replay",
            data: {
                method: selectedRequest.method,
                payload,
            },
        } as any);
    };

    return (
        <div className={styles.page}>
            <aside className={styles.sidebar}>
                <section className={`card ${styles.panel}`}>
                    <div className={styles.controls}>
                        <div className={styles.field}>
                            <label>最近展示组数</label>
                            <input
                                className="form-input"
                                type="number"
                                min={1}
                                max={100}
                                value={displayCount}
                                onChange={(e) => setDisplayCount(Math.min(100, Math.max(1, Number(e.target.value) || 10)))}
                            />
                        </div>
                        <div className={styles.field}>
                            <label className={styles.toggleRow}>
                                <input
                                    type="checkbox"
                                    checked={enabled}
                                    onChange={(e) => setEnabled(e.target.checked)}
                                />
                                开启封包测试
                            </label>
                            <div className={styles.hint}>默认关闭。关闭时后端不会继续向前端发送封包数据。</div>
                        </div>
                        <div className={styles.field}>
                            <label>屏蔽 method</label>
                            <textarea
                                className={`form-input ${styles.methodEditor}`}
                                rows={5}
                                value={blockedMethodsText}
                                onChange={(e) => setBlockedMethodsText(e.target.value)}
                                placeholder=".lq.Route.heartbeat"
                            />
                            <div className={styles.hint}>支持按行或逗号分隔。匹配到的 method 会在后端直接屏蔽，不再发给前端。</div>
                        </div>
                        <label className={styles.toggleRow}>
                            <input
                                type="checkbox"
                                checked={autoShowNew}
                                onChange={(e) => setAutoShowNew(e.target.checked)}
                            />
                            收到新封包时立即切换到最新封包
                        </label>
                        <div className={styles.field}>
                            <label>按流筛选</label>
                            <select
                                className="form-input"
                                value={selectedFlow}
                                onChange={(e) => setSelectedFlow(e.target.value)}
                            >
                                <option value="all">全部流</option>
                                {flowOptions.map((flow) => (
                                    <option key={String(flow.id)} value={String(flow.id)}>
                                        {flow.peer_key || `flow-${flow.id}`}
                                    </option>
                                ))}
                            </select>
                        </div>
                    </div>
                </section>

                <section className={`card ${styles.panel}`}>
                    <div className={styles.flowEventHeader}>流事件</div>
                    <div className={styles.flowEventList}>
                        {filteredFlowEvents.map((event) => (
                            <div key={`${event.seq}-${event.event}`} className={styles.flowEventItem}>
                                <span className={`${styles.flowEventBadge} ${event.event === "end" ? styles.flowEventEnd : ""}`}>
                                    {event.event === "end" ? "断开" : "建立"}
                                </span>
                                <span className={styles.flowEventText}>
                                    {event.flow?.peer_key || `flow-${event.flow?.id ?? "-"}`}
                                </span>
                                <span className={styles.flowEventTime}>{formatTime(event.ts)}</span>
                            </div>
                        ))}
                        {filteredFlowEvents.length === 0 && <div className={styles.hint}>暂无流事件</div>}
                    </div>
                </section>

                <section className={`card ${styles.panel} ${styles.listPanel}`}>
                    <div className={styles.list}>
                        {visibleGroups.map((group) => (
                            <button
                                key={group.key}
                                className={`${styles.item} ${group.key === selectedKey ? styles.itemActive : ""}`}
                                onClick={() => setSelectedKey(group.key)}
                            >
                                <div className={styles.itemTitle}>{group.method || "(unknown method)"}</div>
                                <div className={styles.itemMeta}>
                                    <span className={styles.badge}>msg_id: {group.msgId ?? "-"}</span>
                                    <span className={styles.badge}>flow: {group.flowPeerKey || group.flowId || "-"}</span>
                                    <span className={styles.badge}>
                                        {group.request ? "发" : "-"} / {group.response ? "收" : "-"}
                                    </span>
                                    <span>{formatTime(group.latestTs)}</span>
                                </div>
                            </button>
                        ))}
                        {visibleGroups.length === 0 && (
                            <div className={styles.empty}>当前没有可展示的封包。可以检查屏蔽列表，或等待新的封包到来。</div>
                        )}
                    </div>
                </section>
            </aside>

            <section className={`card ${styles.detail}`}>
                {!selectedGroup && <div className={styles.empty}>选择左侧封包组后，可在这里查看请求/响应详情并重放请求。</div>}

                {selectedGroup && (
                    <>
                        <div className={styles.detailHeader}>
                            <div className={styles.detailTitle}>
                                <div><b>{selectedGroup.method || "(unknown method)"}</b></div>
                                <div className={styles.hint}>
                                    msg_id: {selectedGroup.msgId ?? "-"} | 最近时间: {formatTime(selectedGroup.latestTs)}
                                </div>
                                <div className={styles.hint}>
                                    flow: {selectedGroup.flowPeerKey || selectedGroup.flowId || "-"}
                                </div>
                            </div>
                            <button className="btn" onClick={replaySelected} disabled={!selectedRequest}>
                                重放请求
                            </button>
                        </div>

                        <div className={styles.jsonGrid}>
                            <div className={styles.jsonCard}>
                                <div className={styles.jsonHeader}>
                                    <b>发送封包</b>
                                    <span className={styles.hint}>{selectedRequest ? formatTime(selectedRequest.ts) : "无"}</span>
                                </div>
                                <textarea
                                    className={`${styles.jsonBody} ${styles.editor}`}
                                    value={
                                        selectedRequest
                                            ? requestEditorText
                                            : "当前分组没有可重放的请求封包。"
                                    }
                                    onChange={(e) => setRequestEditorText(e.target.value)}
                                    readOnly={!selectedRequest}
                                    spellCheck={false}
                                />
                            </div>

                            <div className={styles.jsonCard}>
                                <div className={styles.jsonHeader}>
                                    <b>接收封包</b>
                                    <span className={styles.hint}>
                                        {selectedResponse
                                            ? formatTime(selectedResponse.ts)
                                            : selectedFallback
                                                ? formatTime(selectedFallback.ts)
                                                : "无"}
                                    </span>
                                </div>
                                <pre className={styles.jsonBody}>
                                    {selectedResponse
                                        ? JSON.stringify(selectedResponse.data, null, 2)
                                        : selectedFallback
                                            ? JSON.stringify(selectedFallback.data, null, 2)
                                            : "暂无内容。"}
                                </pre>
                            </div>
                        </div>
                    </>
                )}
            </section>
        </div>
    );
}
