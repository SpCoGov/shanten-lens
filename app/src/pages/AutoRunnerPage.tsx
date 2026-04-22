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
    setTargetValue,
    type AutoRunnerRemakeRecord,
    type TargetItem,
    useAutoRunner,
} from "../lib/autoRunnerStore";
import {useRegistry} from "../lib/registryStore";
import AmuletEditorModal, {type EditedAmulet} from "../components/AmuletEditorModal";
import BadgePickerModal from "../components/BadgePickerModal";
import AmuletCard from "../components/AmuletCard";
import Modal from "../components/Modal";
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

function formatAutoLevel(level?: number | null): string {
    if (!level || level <= 0) return "-";
    const a = Math.floor(level / 100);
    const b = level % 100;
    return `${a}-${b}`;
}

export default function AutoRunnerPage() {
    const {t} = useTranslation();
    const {config, status} = useAutoRunner();
    const {badgeById, amuletById} = useRegistry();

    const [saving, setSaving] = React.useState(false);
    const [refreshing, setRefreshing] = React.useState(false);
    const [openAmuletEditor, setOpenAmuletEditor] = React.useState(false);
    const [openBadgePicker, setOpenBadgePicker] = React.useState(false);
    const [detailTargetIndex, setDetailTargetIndex] = React.useState<number | null>(null);
    const [selectedRecordSeq, setSelectedRecordSeq] = React.useState<number | null>(null);
    const [levelText, setLevelText] = React.useState<string>(formatLevelNum(config.cutoff_level));

    const working = Boolean(status.running);

    const [, forceTick] = React.useState(0);
    React.useEffect(() => {
        if (!status.running) return;
        const timer = setInterval(() => forceTick((x) => x + 1), 1000);
        return () => clearInterval(timer);
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
        const judgeText = target.kind === "amulet" ? t("autorun.target_judge_amulet") : t("autorun.target_judge_badge");

        const valueBox = (
            <label className="target-value-box" onClick={(e) => e.stopPropagation()}>
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
                    onClick={(e) => e.stopPropagation()}
                    style={{width: 90}}
                    title={t("autorun.target_value_title")}
                />
            </label>
        );

        const actionBar = (
            <div className="target-card-actions">
                {valueBox}
                <button className="nav-btn" onClick={(e) => {
                    e.stopPropagation();
                    removeTargetAt(idx);
                }}>
                    {t("autorun.btn_delete_target")}
                </button>
            </div>
        );

        if (target.kind === "amulet") {
            const rawId = target.id * 10 + (target.plus ? 1 : 0);
            const effectItem = {id: rawId, volume: 1, badge: target.badge != null ? {id: target.badge} : undefined} as any;
            const amuletName = amuletById.get(target.id)?.name ?? `ID ${target.id}`;
            const badgeName = target.badge != null ? (badgeById.get(target.badge)?.name ?? String(target.badge)) : t("autorun.target_any_badge");

            return (
                <button
                    key={idx}
                    type="button"
                    className="panel target-card target-card-button"
                    style={{padding: 10}}
                    title={judgeText}
                    onClick={() => setDetailTargetIndex(idx)}
                >
                    <div className="target-card-main">
                        <AmuletCard item={effectItem} scale={0.7}/>
                        <div className="target-card-copy">
                            <div><b>{t("autorun.target_amulet_label")}</b>{`：${amuletName}${target.plus ? "+" : ""}`}</div>
                            <div><b>{t("autorun.target_badge_label")}</b>{`：${badgeName}`}</div>
                        </div>
                    </div>
                    {actionBar}
                </button>
            );
        }

        const badgeName = badgeById.get(target.id)?.name ?? `ID ${target.id}`;
        const icon = `/assets/badge/badge_${target.id}.png`;
        return (
            <button
                key={idx}
                type="button"
                className="panel target-card target-card-button"
                style={{padding: 10}}
                title={judgeText}
                onClick={() => setDetailTargetIndex(idx)}
            >
                <div className="target-card-main">
                    <img src={icon} alt={badgeName} style={{width: 64, height: 64}} draggable={false}/>
                    <div className="target-card-copy">
                        <div><b>印章</b>{`：${badgeName}`}</div>
                    </div>
                </div>
                {actionBar}
            </button>
        );
    };

    const detailTarget = detailTargetIndex != null ? config.targets[detailTargetIndex] : null;
    const detailTitle = React.useMemo(() => {
        if (!detailTarget) return "";
        if (detailTarget.kind === "amulet") {
            const amuletName = amuletById.get(detailTarget.id)?.name ?? `ID ${detailTarget.id}`;
            return `判定说明：${amuletName}${detailTarget.plus ? "+" : ""}`;
        }
        return `判定说明：${badgeById.get(detailTarget.id)?.name ?? `ID ${detailTarget.id}`}`;
    }, [detailTarget, amuletById, badgeById]);
    const detailBody = detailTarget
        ? detailTarget.kind === "amulet"
            ? t("autorun.target_judge_amulet")
            : t("autorun.target_judge_badge")
        : "";

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
    }, [working, status.game_ready, status.game_ready_code, status.game_ready_reason, t]);

    const opInterval = Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 1000;
    const remakeRecords = React.useMemo(
        () => [...(status.remake_records ?? [])].sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0)),
        [status.remake_records],
    );
    const bestRecord = status.best_remake_record ?? null;

    React.useEffect(() => {
        if (selectedRecordSeq != null) return;
        if (bestRecord?.seq != null) {
            setSelectedRecordSeq(bestRecord.seq);
        } else if (remakeRecords[0]?.seq != null) {
            setSelectedRecordSeq(remakeRecords[0].seq);
        }
    }, [bestRecord, remakeRecords, selectedRecordSeq]);

    const selectedRecord = React.useMemo(() => {
        if (selectedRecordSeq == null) return bestRecord ?? remakeRecords[0] ?? null;
        return remakeRecords.find((record) => record.seq === selectedRecordSeq) ?? bestRecord ?? remakeRecords[0] ?? null;
    }, [bestRecord, remakeRecords, selectedRecordSeq]);

    const reasonText = React.useCallback((reason?: string) => {
        if (reason === "impossible") return t("autorun.remake_reason_impossible");
        if (reason === "cutoff_no_shop_options") return t("autorun.remake_reason_cutoff_no_shop_options");
        if (reason === "cutoff_cannot_afford") return t("autorun.remake_reason_cutoff_cannot_afford");
        return reason || "-";
    }, [t]);

    const describeRecord = React.useCallback((record: AutoRunnerRemakeRecord | null) => {
        if (!record) return "-";
        return t("autorun.remake_record_summary", {
            seq: record.seq ?? "-",
            run: record.run_index ?? "-",
            value: record.target_value ?? 0,
            count: record.amulet_count ?? 0,
            level: formatAutoLevel(record.level),
        });
    }, [t]);

    const renderRecordAmulets = (record: AutoRunnerRemakeRecord | null) => {
        const effectList = record?.effect_list ?? [];
        if (!record) {
            return <div className="hint">{t("autorun.remake_empty")}</div>;
        }
        if (effectList.length === 0) {
            return <div className="hint">{t("autorun.remake_no_amulets")}</div>;
        }
        return (
            <div style={{display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(92px, 1fr))", gap: 10}}>
                {effectList.map((item, idx) => {
                    const rawId = Number(item?.id ?? 0);
                    const regId = Math.floor(rawId / 10);
                    const amuletName = amuletById.get(regId)?.name ?? `ID ${regId || rawId}`;
                    return (
                        <div key={`${record.seq}-${item?.uid ?? idx}`} style={{display: "grid", gap: 6, justifyItems: "center", minWidth: 0}}>
                            <AmuletCard item={item} scale={0.56}/>
                            <div className="hint" style={{fontSize: 12, textAlign: "center", width: "100%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap"}} title={`${amuletName}`}>
                                {amuletName}
                            </div>
                        </div>
                    );
                })}
            </div>
        );
    };

    return (
        <div className="settings-wrap wide-page" style={{paddingBlock: 16}}>
            <h2 className="title">{t("autorun.title")}</h2>

            <div className="page-stack">
                <div className="responsive-two-col">
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
                </div>

                <section className="panel">
                    <div className="panel-title">{t("autorun.section_remake_records_title")}</div>
                    <div style={{display: "flex", gap: 12, flexWrap: "wrap", marginBottom: 10}}>
                        <span className="badge">{t("autorun.remake_record_count", {count: remakeRecords.length})}</span>
                        <span className="badge ok">{t("autorun.remake_best_value", {value: bestRecord?.target_value ?? 0})}</span>
                        <span className="badge">{t("autorun.remake_best_record", {text: describeRecord(bestRecord)})}</span>
                    </div>

                    {remakeRecords.length === 0 ? (
                        <div className="hint">{t("autorun.remake_empty")}</div>
                    ) : (
                        <div style={{display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))", gap: 12, alignItems: "start"}}>
                            <div style={{display: "grid", gap: 6, maxHeight: 360, overflowY: "auto", paddingRight: 4}}>
                                {remakeRecords.map((record) => {
                                    const active = selectedRecord?.seq === record.seq;
                                    const isBest = bestRecord?.seq === record.seq;
                                    return (
                                        <button
                                            key={record.seq}
                                            type="button"
                                            className={`nav-btn ${active ? "active" : ""}`}
                                            onClick={() => setSelectedRecordSeq(record.seq)}
                                            style={{
                                                display: "grid",
                                                gridTemplateColumns: "auto 1fr auto",
                                                gap: 8,
                                                alignItems: "center",
                                                textAlign: "left",
                                                width: "100%",
                                            }}
                                            title={reasonText(record.reason)}
                                        >
                                            <span>#{record.seq}</span>
                                            <span style={{overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap"}}>
                                                {t("autorun.remake_record_row", {
                                                    run: record.run_index ?? "-",
                                                    value: record.target_value ?? 0,
                                                    level: formatAutoLevel(record.level),
                                                })}
                                            </span>
                                            {isBest ? <span className="badge ok">{t("autorun.remake_best_marker")}</span> : null}
                                        </button>
                                    );
                                })}
                            </div>

                            <div style={{display: "grid", gap: 10, minWidth: 0}}>
                                <div style={{display: "flex", gap: 8, flexWrap: "wrap"}}>
                                    <span className="badge">{describeRecord(selectedRecord)}</span>
                                    <span className="badge">{t("autorun.remake_reason_label", {reason: reasonText(selectedRecord?.reason)})}</span>
                                    <span className="badge">
                                        {t("autorun.remake_time_label", {time: selectedRecord?.ts ? new Date(selectedRecord.ts).toLocaleString() : "-"})}
                                    </span>
                                </div>
                                <div style={{maxHeight: 360, overflowY: "auto", paddingRight: 4}}>
                                    {renderRecordAmulets(selectedRecord)}
                                </div>
                            </div>
                        </div>
                    )}
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

                    <div className="target-list">
                        {config.targets.length === 0 ? (
                            <div className="hint">{t("autorun.empty_targets")}</div>
                        ) : (
                            config.targets.map((item, i) => renderTarget(item, i))
                        )}
                    </div>
                </section>

                <div className="responsive-two-col">
                    <section className="panel">
                        <div className="panel-title">{t("autorun.section_cutoff_title")}</div>
                        <div className="rows">
                            <div className="row flush" style={{gridTemplateColumns: "max-content max-content", alignItems: "center"}}>
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
                            <Trans i18nKey="autorun.hint_modes"/>
                        </p>
                    </section>
                </div>

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
                                        <input className="form-input" placeholder="smtp.example.com" value={email.host ?? ""} onChange={(e) => patchEmail({host: e.target.value.trim()})}/>
                                    </label>

                                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                        <span>{t("autorun.label_smtp_port")}</span>
                                        <input className="form-input" type="number" min={1} max={65535} value={Number(email.port ?? 587)} onChange={(e) => patchEmail({port: Math.max(1, Math.min(65535, Number(e.target.value || 587)))})}/>
                                    </label>

                                    <label className="row" style={{gridTemplateColumns: "auto auto"}}>
                                        <span>{t("autorun.label_smtp_ssl")}</span>
                                        <input className="form-checkbox" type="checkbox" checked={!!email.ssl} onChange={(e) => patchEmail({ssl: e.target.checked})}/>
                                    </label>
                                </div>

                                <div style={rowCols("minmax(220px, 320px) minmax(220px, 320px)")}>
                                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                        <span>{t("autorun.label_email_from")}</span>
                                        <input className="form-input" type="email" placeholder="sender@example.com" value={email.from ?? ""} onChange={(e) => patchEmail({from: e.target.value.trim()})}/>
                                    </label>

                                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                        <span>{t("autorun.label_email_pass")}</span>
                                        <input className="form-input" type="password" placeholder={t("autorun.label_email_pass_placeholder")} value={email.pass ?? ""} onChange={(e) => patchEmail({pass: e.target.value})}/>
                                    </label>
                                </div>

                                <div style={rowCols("minmax(220px, 320px)")}>
                                    <label className="row" style={{gridTemplateColumns: "auto 1fr"}}>
                                        <span>{t("autorun.label_email_to")}</span>
                                        <input className="form-input" type="email" placeholder="you@example.com" value={email.to ?? ""} onChange={(e) => patchEmail({to: e.target.value.trim()})}/>
                                    </label>
                                </div>

                                <div className="toolbar" style={{gap: 8, marginTop: 6, flexWrap: "wrap" as const}}>
                                    <button
                                        className="nav-btn"
                                        onClick={() => ws.send({type: "autorun_control", data: {action: "notify_test_email"}})}
                                        disabled={!email.enabled || !(email.host && email.port) || !(email.from || "").includes("@") || !(email.to || "").includes("@") || !email.pass}
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

                <div className="page-footer-actions">
                    <button className="nav-btn" onClick={onSave} disabled={saving}>
                        {saving ? t("autorun.btn_saving") : t("autorun.btn_save")}
                    </button>
                </div>
            </div>

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
            <Modal open={detailTarget != null} onClose={() => setDetailTargetIndex(null)} title={detailTitle} width={560}>
                <div style={{lineHeight: 1.7}}>{detailBody}</div>
            </Modal>
        </div>
    );
}
