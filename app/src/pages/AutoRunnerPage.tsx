import React from "react";
import { ws } from "../lib/ws";
import {
    addTargetAmulet,
    addTargetBadge,
    type AutoRunnerStatus,
    formatLevelNum,
    parseLevelText,
    patchAutoConfig,
    removeTargetAt,
    setAutoStatus,
    type TargetItem,
    useAutoRunner,
} from "../lib/autoRunnerStore";
import { useRegistry } from "../lib/registryStore";
import AmuletEditorModal, { type EditedAmulet } from "../components/AmuletEditorModal";
import BadgePickerModal from "../components/BadgePickerModal";
import AmuletCard from "../components/AmuletCard";

function formatDuration(ms: number): string {
    if (!ms || ms < 0) ms = 0;
    const s = Math.floor(ms / 1000);
    const hh = Math.floor(s / 3600);
    const mm = Math.floor((s % 3600) / 60);
    const ss = s % 60;
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(hh)}:${pad(mm)}:${pad(ss)}`;
}

export default function AutoRunnerPage() {
    const { config, status } = useAutoRunner();
    const { badgeById, amuletById } = useRegistry();

    const [saving, setSaving] = React.useState(false);
    const [working, setWorking] = React.useState<boolean>(false);
    const [refreshing, setRefreshing] = React.useState(false);

    const [openAmuletEditor, setOpenAmuletEditor] = React.useState(false);
    const [openBadgePicker, setOpenBadgePicker] = React.useState(false);

    const [levelText, setLevelText] = React.useState<string>(formatLevelNum(config.cutoff_level));

    // 每秒重渲染一次，让 elapsed_ms 走动（仅显示，不做加法）
    const [, forceTick] = React.useState(0);
    React.useEffect(() => {
        if (!status.running) return;
        const t = setInterval(() => forceTick((x) => x + 1), 1000);
        return () => clearInterval(t);
    }, [status.running]);

    React.useEffect(() => {
        setLevelText(formatLevelNum(config.cutoff_level));
    }, [config.cutoff_level]);

    React.useEffect(() => {
        const offPkt = ws.onPacket((pkt) => {
            if (pkt.type === "autorun_status") {
                setAutoStatus(pkt.data as AutoRunnerStatus);
                setWorking(Boolean(pkt.data?.running));
                setRefreshing(false);
            }
            if (pkt.type === "autorun_control_result" && pkt.data) {
                const d = pkt.data as { ok: boolean; reason?: string; requires_confirmation?: boolean };
                if (d.requires_confirmation) {
                    // 这里原来有行内确认条逻辑，如需保留可按你的实现继续
                } else {
                    // ...
                }
                setRefreshing(false);
            }
        });
        return () => offPkt();
    }, []);

    const onSave = React.useCallback(() => {
        setSaving(true);
        try {
            ws.send({ type: "edit_config", data: { autorun: config } });
        } finally {
            setSaving(false);
        }
    }, [config]);

    const start = React.useCallback(() => {
        ws.send({ type: "autorun_control", data: { action: "start" } });
    }, []);

    const stop = React.useCallback(() => {
        ws.send({ type: "autorun_control", data: { action: "stop" } });
    }, []);

    const renderTarget = (t: TargetItem, idx: number) => {
        if (t.kind === "amulet") {
            const rawId = t.id * 10 + (t.plus ? 1 : 0);
            const effectItem = { id: rawId, volume: 1, badge: t.badge != null ? { id: t.badge } : undefined } as any;
            const amu = amuletById.get(t.id) || null;
            const amuName = amu?.name ?? `ID ${t.id}`;

            return (
                <div
                    key={idx}
                    style={{
                        border: "1px solid #ddd",
                        borderRadius: 12,
                        background: "#fff",
                        padding: 10,
                        display: "flex",
                        alignItems: "center",
                        gap: 10,
                    }}
                >
                    <AmuletCard item={effectItem} scale={0.7} />
                    <div style={{ lineHeight: 1.6 }}>
                        <div>
                            <b>护身符</b>：{amuName}
                            {t.plus ? "+" : ""}
                        </div>
                        <div>要求印章：{t.badge != null ? (badgeById.get(t.badge)?.name ?? t.badge) : "任意/无均可"}</div>
                        <div style={{ color: "#888", fontSize: 12 }}>
                            判定：若设置了印章则需“拥有该印章的护身符”；未设置则该护身符无或任意印章均满足。
                        </div>
                    </div>
                    <div style={{ flex: 1 }} />
                    <button className="nav-btn" onClick={() => removeTargetAt(idx)}>
                        删除
                    </button>
                </div>
            );
        } else {
            const badge = badgeById.get(t.id) || null;
            const icon = `/assets/badge/badge_${t.id}.png`;
            return (
                <div
                    key={idx}
                    style={{
                        border: "1px solid #ddd",
                        borderRadius: 12,
                        background: "#fff",
                        padding: 10,
                        display: "flex",
                        alignItems: "center",
                        gap: 12,
                    }}
                >
                    <img src={icon} alt={badge?.name ?? String(t.id)} style={{ width: 64, height: 64 }} draggable={false} />
                    <div style={{ lineHeight: 1.6 }}>
                        <div>
                            <b>印章</b>：{badge ? `${badge.name}（ID ${t.id}）` : `ID ${t.id}`}
                        </div>
                        <div style={{ color: "#888", fontSize: 12 }}>判定：拥有该印章即可计数。</div>
                    </div>
                    <div style={{ flex: 1 }} />
                    <button className="nav-btn" onClick={() => removeTargetAt(idx)}>
                        删除
                    </button>
                </div>
            );
        }
    };

    const elapsedDisplay = formatDuration(status.elapsed_ms ?? 0);

    const disabledReason = React.useMemo(() => {
        if (working) return "已在运行";
        if (status.game_ready === false) {
            if (status.game_ready_code === "GAME_NOT_READY") return "游戏未启动/流程未就绪";
            if (status.game_ready_code === "PROBE_TIMEOUT") return "连接超时（请检查游戏/代理）";
            return status.game_ready_reason || "未就绪";
        }
        if (status.game_ready === undefined || status.game_ready_code === "NOT_PROBED") return "未探测";
        return "";
    }, [working, status.game_ready, status.game_ready_code, status.game_ready_reason]);

    const opInterval = Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 1000;

    return (
        <div style={{ padding: 16 }}>
            <h2 style={{ margin: "8px 0 12px" }}>自动化</h2>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>运行信息</div>
                <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
                    <span className="badge">已运行时长：{elapsedDisplay}</span>
                    <span className="badge">已运行局数：{status.runs ?? 0}</span>
                    <span className="badge">历史最高目标数：{status.best_achieved_count ?? 0}</span>
                    <span className="badge" title="run_tick 的调度间隔（毫秒）">
                        操作间隔：{opInterval}ms
                    </span>
                </div>
                <div style={{ marginTop: 8, color: "#666", fontSize: 13 }}>
                    <div>当前步骤：{status.current_step ?? "-"}</div>
                    <div>最近错误：{status.last_error ?? "-"}</div>
                    <div>启动时间：{status.started_at ? new Date(status.started_at).toLocaleString() : "-"}</div>
                </div>
            </section>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>运行控制</div>

                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                    <button
                        className="nav-btn"
                        onClick={start}
                        disabled={Boolean(disabledReason) || status.mode === "step"}
                        title={
                            status.mode === "step"
                                ? "手动单步模式下无需启动，直接点“下一步”"
                                : disabledReason || undefined
                        }
                    >
                        {working ? "运行中…" : "启动"}
                    </button>

                    <button className="nav-btn" onClick={stop} disabled={!working}>
                        停止
                    </button>

                    <button
                        className="nav-btn"
                        onClick={() => {
                            setRefreshing(true);
                            ws.send({ type: "autorun_control", data: { action: "probe" } });
                        }}
                        disabled={refreshing}
                        title="立即手动探测游戏是否就绪"
                    >
                        {refreshing ? "刷新中…" : "刷新状态"}
                    </button>

                    <label style={{ display: "inline-flex", alignItems: "center", gap: 8, marginLeft: 6 }}>
                        <span style={{ color: "#555", whiteSpace: "nowrap" }}>操作间隔</span>
                        <input
                            type="number"
                            min={10}
                            max={5000}
                            step={10}
                            value={Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 50}
                            onChange={(e) => {
                                const raw = Number(e.target.value || 0);
                                const clamped = Math.max(10, Math.min(5000, Math.round(raw)));
                                patchAutoConfig({ op_interval_ms: clamped });
                            }}
                            title="run_tick 的调度间隔（毫秒）。建议 1000（1秒）；过低可能被暂时封禁IP。"
                            style={{ width: 120, padding: "6px 8px", borderRadius: 8, border: "1px solid #ddd" }}
                        />
                        <span style={{ color: "#888", fontSize: 12, whiteSpace: "nowrap" }}>ms</span>
                    </label>

                    <span className={`badge ${working ? "ok" : "down"}`}>{working ? "运行中" : "已停止"}</span>

                    {(() => {
                        const ready = status.preferred_flow_ready;
                        const cls = ready === true ? "ok" : ready === false ? "down" : "";
                        const text = ready === true ? "业务流：已选定" : ready === false ? "业务流：未选定" : "业务流：未知";
                        const tip = status.preferred_flow_peer
                            ? `peer: ${status.preferred_flow_peer}`
                            : (ready === false ? "未绑定游戏业务流" : undefined);
                        return (
                            <span className={`badge ${cls}`} title={tip}>{text}</span>
                        );
                    })()}
                </div>

                <p style={{ marginTop: 8, color: "#666", fontSize: 13, lineHeight: 1.5 }}>
                    达成下方的“结束条件”即停止；若到达“截止关卡”仍未达成，则自动重开。
                </p>
            </section>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>结束条件</div>

                <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap", marginBottom: 12 }}>
                    <span style={{ color: "#555" }}>目标数量</span>
                    <input
                        type="number"
                        min={1}
                        value={Number(config.end_count ?? 1)}
                        onChange={(e) => patchAutoConfig({ end_count: Math.max(1, Number(e.target.value || 1)) })}
                        style={{ width: 100, padding: "6px 8px", borderRadius: 8, border: "1px solid #ddd" }}
                    />
                    <span style={{ color: "#888", fontSize: 12 }}>（集齐该数量的目标即结束）</span>
                </div>

                <div style={{ display: "flex", gap: 8, marginBottom: 10, flexWrap: "wrap" }}>
                    <button className="nav-btn" onClick={() => setOpenAmuletEditor(true)}>
                        添加护身符目标
                    </button>
                    <button className="nav-btn" onClick={() => setOpenBadgePicker(true)}>
                        添加印章目标
                    </button>
                </div>

                <div style={{ display: "grid", gap: 10 }}>
                    {config.targets.length === 0 ? (
                        <div style={{ color: "#888" }}>尚未添加目标。</div>
                    ) : (
                        config.targets.map((t, i) => renderTarget(t, i))
                    )}
                </div>
            </section>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>截止关卡</div>
                <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                    <span style={{ color: "#555" }}>关卡</span>
                    <input
                        value={levelText}
                        placeholder="例如 1-1"
                        onChange={(e) => {
                            const s = e.target.value;
                            setLevelText(s);
                            const n = parseLevelText(s);
                            patchAutoConfig({ cutoff_level: n ?? 0 });
                        }}
                        style={{ width: 130, padding: "6px 8px", borderRadius: 8, border: "1px solid #ddd" }}
                    />
                </div>
                <p style={{ marginTop: 8, color: "#888", fontSize: 12 }}>若到该关卡仍未达成“目标数量”，则本局结束并自动重开。</p>
            </section>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>执行模式</div>
                <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
                    <label style={{ display: "inline-flex", alignItems: "center", gap: 8, whiteSpace: "nowrap" }}>
                        <span>模式</span>
                        <select
                            value={status.mode ?? "continuous"}
                            onChange={(e) => ws.send({ type: "autorun_control", data: { action: "set_mode", mode: e.target.value } })}
                            style={{ padding: "6px 8px", borderRadius: 8, border: "1px solid #ddd" }}
                        >
                            <option value="continuous">持续运行</option>
                            <option value="step">手动单步</option>
                        </select>
                    </label>

                    <button
                        className="nav-btn"
                        onClick={() => ws.send({ type: "autorun_control", data: { action: "step" } })}
                        disabled={status.mode !== "step"}
                        title={status.mode !== "step" ? "切换到“手动单步”模式后可用" : undefined}
                    >
                        下一步
                    </button>
                </div>
                <p style={{ marginTop: 8, color: "#888", fontSize: 12 }}>
                    · 持续运行：后台自动循环执行。<br />
                    · 手动单步：不自动运行，点击“下一步”仅执行一次调度。
                </p>
            </section>

            <section
                style={{
                    border: "1px solid var(--border,#ddd)",
                    borderRadius: 12,
                    background: "#fff",
                    padding: 12,
                    marginBottom: 12,
                }}
            >
                <div style={{ fontWeight: 600, marginBottom: 6 }}>邮件通知</div>

                {(() => {
                    const email = (config.email_notify ?? {
                        enabled: false,
                        host: "",
                        port: 587,
                        ssl: false,
                        from: "",
                        pass: "",
                        to: "",
                    });

                    const patchEmail = (kv: Partial<typeof email>) =>
                        patchAutoConfig({ email_notify: { ...email, ...kv } });

                    const box = { width: "100%", maxWidth: 280, padding: "6px 8px", borderRadius: 8, border: "1px solid #ddd" } as const;
                    const field = { display: "grid", gap: 6, justifyItems: "start" } as const;
                    const rowGrid = (cols: string) => ({
                        display: "grid",
                        gridTemplateColumns: cols,
                        columnGap: 12,
                        rowGap: 10,
                        alignItems: "start",
                    } as const);

                    return (
                        <div style={{ display: "grid", gap: 10 }}>
                            <label style={{ display: "inline-flex", alignItems: "center", gap: 8 }}>
                                <input
                                    type="checkbox"
                                    checked={!!email.enabled}
                                    onChange={(e) => patchEmail({ enabled: e.target.checked })}
                                />
                                <span>启用邮件通知（SMTP）</span>
                            </label>

                            <div style={rowGrid("minmax(220px, 320px) 140px 140px")}>
                                <label style={field}>
                                    <span style={{ color: "#555" }}>SMTP 服务器</span>
                                    <input
                                        placeholder="smtp.example.com"
                                        value={email.host ?? ""}
                                        onChange={(e) => patchEmail({ host: e.target.value.trim() })}
                                        style={box}
                                    />
                                </label>

                                <label style={field}>
                                    <span style={{ color: "#555" }}>端口</span>
                                    <input
                                        type="number"
                                        min={1}
                                        max={65535}
                                        value={Number(email.port ?? 587)}
                                        onChange={(e) =>
                                            patchEmail({ port: Math.max(1, Math.min(65535, Number(e.target.value || 587))) })
                                        }
                                        style={{ ...box, maxWidth: 160 }}
                                    />
                                </label>

                                <label style={field}>
                                    <span style={{ color: "#555" }}>SSL/TLS</span>
                                    <div style={{ display: "flex", alignItems: "center", gap: 8, height: 36 }}>
                                        <input
                                            type="checkbox"
                                            checked={!!email.ssl}
                                            onChange={(e) => patchEmail({ ssl: e.target.checked })}
                                            style={{ transform: "translateY(1px)" }} // 微调视觉居中
                                        />
                                        <span style={{ fontSize: 12, color: "#666" }}>{email.ssl ? "开启" : "关闭"}</span>
                                    </div>
                                </label>
                            </div>

                            <div style={rowGrid("minmax(220px, 320px) minmax(220px, 320px)")}>
                                <label style={field}>
                                    <span style={{ color: "#555" }}>发件邮箱</span>
                                    <input
                                        type="email"
                                        placeholder="sender@example.com"
                                        value={email.from ?? ""}
                                        onChange={(e) => patchEmail({ from: e.target.value.trim() })}
                                        style={box}
                                    />
                                </label>

                                <label style={field}>
                                    <span style={{ color: "#555" }}>密码/授权码</span>
                                    <input
                                        type="password"
                                        placeholder="密码"
                                        value={email.pass ?? ""}
                                        onChange={(e) => patchEmail({ pass: e.target.value })}
                                        style={box}
                                    />
                                </label>
                            </div>

                            <div style={rowGrid("minmax(220px, 320px)")}>
                                <label style={field}>
                                    <span style={{ color: "#555" }}>收件邮箱</span>
                                    <input
                                        type="email"
                                        placeholder="you@example.com"
                                        value={email.to ?? ""}
                                        onChange={(e) => patchEmail({ to: e.target.value.trim() })}
                                        style={box}
                                    />
                                </label>
                            </div>

                            <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 6, flexWrap: "wrap" }}>
                                <button
                                    className="nav-btn"
                                    onClick={() => ws.send({ type: "autorun_control", data: { action: "notify_test_email" } })}
                                    disabled={
                                        !email.enabled ||
                                        !(email.host && email.port) ||
                                        !(email.from || "").includes("@") ||
                                        !(email.to || "").includes("@") ||
                                        !(email.pass)
                                    }
                                    title={
                                        !email.enabled ? "请先启用邮件通知"
                                            : !(email.host && email.port) ? "请填写服务器与端口"
                                                : !(email.from || "").includes("@") ? "请填写发件邮箱"
                                                    : !(email.pass) ? "请填写密码"
                                                        : !(email.to || "").includes("@") ? "请填写收件邮箱"
                                                            : undefined
                                    }
                                >
                                    发送测试邮件
                                </button>
                            </div>
                        </div>
                    );
                })()}
            </section>

            <button className="nav-btn" onClick={onSave} disabled={saving}>
                {saving ? "保存中…" : "保存配置"}
            </button>

            <AmuletEditorModal
                open={openAmuletEditor}
                onClose={() => setOpenAmuletEditor(false)}
                onConfirm={(data: EditedAmulet) => {
                    addTargetAmulet({ id: data.id, plus: data.plus, badge: data.badge ?? null });
                    setOpenAmuletEditor(false);
                }}
            />
            <BadgePickerModal
                open={openBadgePicker}
                onClose={() => setOpenBadgePicker(false)}
                onSelect={(id) => {
                    addTargetBadge(id);
                    setOpenBadgePicker(false);
                }}
            />
        </div>
    );
}