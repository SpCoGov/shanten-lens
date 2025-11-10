import React from "react";
import "../styles/theme.css";
import {ws} from "../lib/ws";
import {
    addTargetAmulet,
    addTargetBadge,
    formatLevelNum,
    parseLevelText,
    patchAutoConfig,
    removeTargetAt,
    type TargetItem,
    useAutoRunner,
    setTargetValue,
} from "../lib/autoRunnerStore";
import {useRegistry} from "../lib/registryStore";
import AmuletEditorModal, {type EditedAmulet} from "../components/AmuletEditorModal";
import BadgePickerModal from "../components/BadgePickerModal";
import AmuletCard from "../components/AmuletCard";
import {Trans, useTranslation} from "react-i18next";

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
    const {t} = useTranslation();
    const {config, status} = useAutoRunner();
    const {badgeById, amuletById} = useRegistry();

    const [saving, setSaving] = React.useState(false);
    const working = Boolean(status.running);
    const [refreshing, setRefreshing] = React.useState(false);

    const [openAmuletEditor, setOpenAmuletEditor] = React.useState(false);
    const [openBadgePicker, setOpenBadgePicker] = React.useState(false);

    const [levelText, setLevelText] = React.useState<string>(formatLevelNum(config.cutoff_level));

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
        if (refreshing) setRefreshing(false);
    }, [status, refreshing]);

    const onSave = React.useCallback(() => {
        setSaving(true);
        try {
            ws.send({type: "edit_config", data: {autorun: config}});
        } finally {
            setSaving(false);
        }
    }, [config]);

    const start = React.useCallback(() => {
        ws.send({type: "autorun_control", data: {action: "start"}});
    }, []);

    const stop = React.useCallback(() => {
        ws.send({type: "autorun_control", data: {action: "stop"}});
    }, []);

    const renderTarget = (target: TargetItem, idx: number) => {
        const value = Math.max(1, Math.floor(Number((target as any).value ?? 1)));
        const valueBox = (
            <label style={{display: "inline-flex", alignItems: "center", gap: 6}}>
                <span>{t("autorun.target_value_label")}</span>
                <input
                    className="form-input"
                    type="number"
                    min={1}
                    step={1}
                    value={value}
                    onChange={(e) => {
                        const v = Math.max(1, Math.floor(Number(e.target.value || 1)));
                        setTargetValue(idx, v);
                    }}
                    style={{width: 90}}
                    title={t("autorun.target_value_title")}
                />
            </label>
        );

        if (target.kind === "amulet") {
            const rawId = target.id * 10 + (target.plus ? 1 : 0);
            const effectItem = {id: rawId, volume: 1, badge: target.badge != null ? {id: target.badge} : undefined} as any;
            const amu = amuletById.get(target.id) || null;
            const amuName = amu?.name ?? `ID ${target.id}`;

            return (
                <div key={idx} className="panel" style={{padding: 10, display: "flex", alignItems: "center", gap: 10}}>
                    <AmuletCard item={effectItem} scale={0.7}/>
                    <div style={{lineHeight: 1.6}}>
                        <div>
                            <b>{t("autorun.target_amulet_label")}</b>：{amuName}
                            {target.plus ? "+" : ""}
                        </div>
                        <div>
                            <b>{t("autorun.target_badge_label")}</b>：{target.badge != null ? (badgeById.get(target.badge)?.name ?? target.badge) : t("autorun.target_any_badge")}
                        </div>
                        <div className="hint">{t("autorun.target_judge_amulet")}</div>
                    </div>
                    <div style={{flex: 1}}/>
                    {valueBox}
                    <button className="nav-btn" onClick={() => removeTargetAt(idx)}>
                        {t("autorun.btn_delete_target")}
                    </button>
                </div>
            );
        } else {
            const badge = badgeById.get(target.id) || null;
            const icon = `/assets/badge/badge_${target.id}.png`;
            return (
                <div key={idx} className="panel" style={{padding: 10, display: "flex", alignItems: "center", gap: 12}}>
                    <img src={icon} alt={badge?.name ?? String(target.id)} style={{width: 64, height: 64}} draggable={false}/>
                    <div style={{lineHeight: 1.6}}>
                        <div>
                            <b>印章</b>：{badge ? `${badge.name}` : `ID ${target.id}`}
                        </div>
                        <div className="hint">{t("autorun.target_judge_badge")}</div>
                    </div>
                    <div style={{flex: 1}}/>
                    {valueBox}
                    <button className="nav-btn" onClick={() => removeTargetAt(idx)}>
                        {t("autorun.btn_delete_target")}
                    </button>
                </div>
            );
        }
    };

    const elapsedDisplay = formatDuration(status.elapsed_ms ?? 0);

    const disabledReason = React.useMemo(() => {
        if (working) return t("autorun.disabled_reason_running");
        if (status.game_ready === false) {
            if (status.game_ready_code === "GAME_NOT_READY") return t("autorun.disabled_reason_game_not_ready");
            if (status.game_ready_code === "PROBE_TIMEOUT") return t("autorun.disabled_reason_probe_timeout");
            return t(status.game_ready_reason ?? "") || t("autorun.disabled_reason_not_ready");
        }
        if (status.game_ready === undefined || status.game_ready_code === "NOT_PROBED") return t("autorun.disabled_reason_not_probed");
        return "";
    }, [working, status.game_ready, status.game_ready_code, status.game_ready_reason]);

    const opInterval = Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 1000;

    return (
        <div className="settings-wrap" style={{padding: 16}}>
            <h2 className="title">{t("autorun.title")}</h2>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_runtime_title")}</div>
                <div style={{display: "flex", gap: 12, flexWrap: "wrap"}}>
                    <span className="badge">{t("autorun.badge_elapsed", {time: elapsedDisplay})}</span>
                    <span className="badge">{t("autorun.badge_runs", {count: status.runs ?? 0})}</span>
                    <span className="badge">{t("autorun.badge_best", {count: status.best_achieved_count ?? 0})}</span>
                    <span className="badge">{t("autorun.badge_interval", {ms: opInterval})}</span>
                </div>
                <div className="hint" style={{marginTop: 8, lineHeight: 1.45}}>
                    <div>{t("autorun.hint_current_step", {text: status.current_step ?? "-"})}</div>
                    <div>{t("autorun.hint_last_error", {text: status.last_error ?? "-"})}</div>
                    <div>{t("autorun.hint_started_at", {time: status.started_at ? new Date(status.started_at).toLocaleString() : "-"})}</div>
                </div>
            </section>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_control_title")}</div>

                <div className="toolbar" style={{gap: 8, flexWrap: "wrap" as const}}>
                    <button
                        className="nav-btn"
                        onClick={start}
                        disabled={Boolean(disabledReason) || status.mode === "step"}
                        title={status.mode === "step" ? t("autorun.tip_no_need_start_in_step") : disabledReason || undefined}
                    >
                        {working ? t("autorun.btn_start_working") : t("autorun.btn_start")}
                    </button>

                    <button className="nav-btn" onClick={stop} disabled={!working}>
                        {t("autorun.btn_stop")}
                    </button>

                    <button
                        className="nav-btn"
                        onClick={() => {
                            setRefreshing(true);
                            ws.send({type: "autorun_control", data: {action: "probe"}});
                        }}
                        disabled={refreshing}
                        title={t("autorun.tip_probe_now")}
                    >
                        {refreshing ? t("autorun.btn_refresh_loading") : t("autorun.btn_refresh")}
                    </button>

                    <label style={{display: "inline-flex", alignItems: "center", gap: 8, marginLeft: 6}}>
                        <span>{t("autorun.label_op_interval")}</span>
                        <input
                            className="form-input"
                            type="number"
                            min={0}
                            max={5000}
                            value={Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 50}
                            onChange={(e) => {
                                const raw = Number(e.target.value || 0);
                                const clamped = Math.max(0, Math.min(5000, Math.round(raw)));
                                patchAutoConfig({op_interval_ms: clamped});
                            }}
                            title={t("autorun.tip_op_interval")}
                            style={{width: 120}}
                        />
                        <span className="hint">{t("autorun.suffix_ms")}</span>
                    </label>

                    <span className={`badge ${working ? "ok" : "down"}`}>{working ? t("autorun.status_running") : t("autorun.status_stopped")}</span>

                    {(() => {
                        const ready = status.preferred_flow_ready;
                        const cls = ready === true ? "ok" : ready === false ? "down" : "";
                        const text = ready === true ? t("autorun.flow_ready") : ready === false ? t("autorun.flow_not_ready") : t("autorun.flow_unknown");
                        const tip = status.preferred_flow_peer
                            ? t("autorun.flow_tip_peer", {peer: status.preferred_flow_peer})
                            : ready === false
                                ? t("autorun.flow_tip_unbound")
                                : undefined;
                        return (
                            <span className={`badge ${cls}`} title={tip}>{text}</span>
                        );
                    })()}
                </div>

                <p className="hint" style={{marginTop: 8, lineHeight: 1.5}}>
                    {t("autorun.control_note")}
                </p>
            </section>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_goal_title")}</div>

                <div className="rows" style={{marginBottom: 8}}>
                    <div className="row" style={{gridTemplateColumns: "auto auto 1fr", alignItems: "center"}}>
                        <label>{t("autorun.label_end_count")}</label>
                        <input
                            className="form-input"
                            type="number"
                            min={1}
                            value={Number(config.end_count ?? 1)}
                            onChange={(e) => patchAutoConfig({end_count: Math.max(1, Number(e.target.value || 1))})}
                            style={{width: 100}}
                        />
                        <span className="hint">{t("autorun.hint_end_count")}</span>
                    </div>
                </div>

                <div className="toolbar" style={{gap: 8, flexWrap: "wrap" as const, marginBottom: 10}}>
                    <button className="nav-btn" onClick={() => setOpenAmuletEditor(true)}>
                        {t("autorun.btn_add_amulet")}
                    </button>
                    <button className="nav-btn" onClick={() => setOpenBadgePicker(true)}>
                        {t("autorun.btn_add_badge")}
                    </button>
                </div>

                <div style={{display: "grid", gap: 10}}>
                    {config.targets.length === 0 ? (
                        <div className="hint">{t("autorun.empty_targets")}</div>
                    ) : (
                        config.targets.map((t, i) => renderTarget(t, i))
                    )}
                </div>
            </section>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_cutoff_title")}</div>
                <div className="rows">
                    <div
                        className="row flush"
                        style={{gridTemplateColumns: "max-content max-content", alignItems: "center"}}
                    >
                        <label>{t("autorun.label_level")}</label>
                        <input
                            className="form-input"
                            value={levelText}
                            placeholder={t("autorun.placeholder_level")}
                            onChange={(e) => {
                                const s = e.target.value;
                                setLevelText(s);
                                const n = parseLevelText(s);
                                patchAutoConfig({cutoff_level: n ?? 0});
                            }}
                            style={{width: 130}}
                        />
                    </div>
                </div>
                <p className="hint">{t("autorun.hint_cutoff")}</p>
            </section>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_mode_title")}</div>
                <div className="toolbar" style={{gap: 8, flexWrap: "wrap" as const}}>
                    <label style={{display: "inline-flex", alignItems: "center", gap: 8, whiteSpace: "nowrap"}}>
                        <span>{t("autorun.label_mode")}</span>
                        <select
                            className="form-input"
                            value={status.mode ?? "continuous"}
                            onChange={(e) => ws.send({type: "autorun_control", data: {action: "set_mode", mode: e.target.value}})}
                            style={{width: 160}}
                        >
                            <option value="continuous">{t("autorun.mode_continuous")}</option>
                            <option value="step">{t("autorun.mode_step")}</option>
                        </select>
                    </label>

                    <button
                        className="nav-btn"
                        onClick={() => ws.send({type: "autorun_control", data: {action: "step"}})}
                        disabled={status.mode !== "step"}
                        title={status.mode !== "step" ? t("autorun.tip_step_only") : undefined}
                    >
                        {t("autorun.btn_next_step")}
                    </button>
                </div>
                <p className="hint" style={{marginTop: 8, lineHeight: 1.5}}>
                    <Trans
                        i18nKey="autorun.hint_modes"
                    />
                </p>
            </section>

            <section className="panel">
                <div className="panel-title">{t("autorun.section_email_title")}</div>

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

                    const patchEmail = (kv: Partial<typeof email>) => patchAutoConfig({email_notify: {...email, ...kv}});

                    const rowCols = (cols: string) =>
                        ({display: "grid", gridTemplateColumns: cols, columnGap: 12, rowGap: 10, alignItems: "start"} as const);

                    return (
                        <div style={{display: "grid", gap: 10}}>
                            <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                                <input
                                    className="form-checkbox"
                                    type="checkbox"
                                    checked={!!email.enabled}
                                    onChange={(e) => patchEmail({enabled: e.target.checked})}
                                />
                                <span>{t("autorun.toggle_email")}</span>
                            </label>

                            <div style={rowCols("minmax(220px, 320px) 160px 160px")}>
                                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                    <span>{t("autorun.label_smtp_host")}</span>
                                    <input
                                        className="form-input"
                                        placeholder="smtp.example.com"
                                        value={email.host ?? ""}
                                        onChange={(e) => patchEmail({host: e.target.value.trim()})}
                                    />
                                </label>

                                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                    <span>{t("autorun.label_smtp_port")}</span>
                                    <input
                                        className="form-input"
                                        type="number"
                                        min={1}
                                        max={65535}
                                        value={Number(email.port ?? 587)}
                                        onChange={(e) => patchEmail({port: Math.max(1, Math.min(65535, Number(e.target.value || 587)))})}
                                    />
                                </label>

                                <label className="row" style={{gridTemplateColumns: "auto auto"}}>
                                    <span>{t("autorun.label_smtp_ssl")}</span>
                                    <input
                                        className="form-checkbox"
                                        type="checkbox"
                                        checked={!!email.ssl}
                                        onChange={(e) => patchEmail({ssl: e.target.checked})}
                                    />
                                </label>
                            </div>

                            <div style={rowCols("minmax(220px, 320px) minmax(220px, 320px)")}>
                                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                    <span>{t("autorun.label_email_from")}</span>
                                    <input
                                        className="form-input"
                                        type="email"
                                        placeholder="sender@example.com"
                                        value={email.from ?? ""}
                                        onChange={(e) => patchEmail({from: e.target.value.trim()})}
                                    />
                                </label>

                                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                    <span>{t("autorun.label_email_pass")}</span>
                                    <input
                                        className="form-input"
                                        type="password"
                                        placeholder={t("autorun.label_email_pass_placeholder")}
                                        value={email.pass ?? ""}
                                        onChange={(e) => patchEmail({pass: e.target.value})}
                                    />
                                </label>
                            </div>

                            <div style={rowCols("minmax(220px, 320px)")}>
                                <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                    <span>{t("autorun.label_email_to")}</span>
                                    <input
                                        className="form-input"
                                        type="email"
                                        placeholder="you@example.com"
                                        value={email.to ?? ""}
                                        onChange={(e) => patchEmail({to: e.target.value.trim()})}
                                    />
                                </label>
                            </div>

                            <div className="toolbar" style={{gap: 8, marginTop: 6, flexWrap: "wrap" as const}}>
                                <button
                                    className="nav-btn"
                                    onClick={() => ws.send({type: "autorun_control", data: {action: "notify_test_email"}})}
                                    disabled={
                                        !email.enabled ||
                                        !(email.host && email.port) ||
                                        !(email.from || "").includes("@") ||
                                        !(email.to || "").includes("@") ||
                                        !email.pass
                                    }
                                    title={
                                        !email.enabled
                                            ? t("autorun.tip_need_enable_email")
                                            : !(email.host && email.port)
                                                ? t("autorun.tip_need_host_port")
                                                : !(email.from || "").includes("@")
                                                    ? t("autorun.tip_need_sender")
                                                    : !email.pass
                                                        ? t("autorun.tip_need_pass")
                                                        : !(email.to || "").includes("@")
                                                            ? t("autorun.tip_need_receiver")
                                                            : undefined
                                    }
                                >
                                    {t("autorun.btn_send_test")}
                                </button>
                            </div>
                        </div>
                    );
                })()}
            </section>

            <button className="nav-btn" onClick={onSave} disabled={saving}>
                {saving ? t("autorun.btn_saving") : t("autorun.btn_save")}
            </button>

            <AmuletEditorModal
                open={openAmuletEditor}
                onClose={() => setOpenAmuletEditor(false)}
                onConfirm={(data: EditedAmulet) => {
                    addTargetAmulet({id: data.id, plus: data.plus, badge: data.badge ?? null, value: 1});
                    setOpenAmuletEditor(false);
                }}
            />
            <BadgePickerModal
                open={openBadgePicker}
                onClose={() => setOpenBadgePicker(false)}
                onSelect={(id) => {
                    addTargetBadge(id, 1);
                    setOpenBadgePicker(false);
                }}
            />
        </div>
    );
}