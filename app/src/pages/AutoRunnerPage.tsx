import React from "react";
import type {GameStateData} from "../lib/gamestate";
import ActionProviderPanel from "../components/ActionProviderPanel";
import RoundDecisionPanel from "../components/RoundDecisionPanel";
import "../styles/theme.css";
import * as backendIpc from "../lib/ipc";
import {
    patchAutoConfig,
    markPlaybookSaved,
    type AutoRunnerOperationRecord,
    type AutoRunnerRemakeRecord,
    useAutoRunner,
} from "../lib/autoRunnerStore";
import {useRegistry} from "../lib/registryStore";
import AmuletCard from "../components/AmuletCard";
import Modal from "../components/Modal";
import {useTranslation} from "react-i18next";
import {formatLevelIdToLabel} from "../lib/levelFormat";
import AutoPlayConfigPage from "./AutoPlayConfigPage";
import DirectConnectionPage from "./DirectConnectionPage";
import styles from "./AutoPlayConfigPage.module.css";
import AutoPlayTargets from "./AutoPlayTargets";
import {openingLevels, validateAutoPlayConfig} from "../lib/autoPlayConfig";

function formatDuration(ms: number): string {
    if (!ms || ms < 0) ms = 0;
    const s = Math.floor(ms / 1000);
    const hh = Math.floor(s / 3600);
    const mm = Math.floor((s % 3600) / 60);
    const ss = s % 60;
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(hh)}:${pad(mm)}:${pad(ss)}`;
}

export default function AutoRunnerPage({currentState}: {currentState: GameStateData | null}) {
    const {t} = useTranslation();
    const [tab, setTab] = React.useState<"strategy" | "monitor" | "connection">("monitor");
    return <div className={`settings-wrap wide-page ${styles.shell}`}>
        <header className={styles.pageHeader}>
            <h2 className="title"><span className="ms" aria-hidden="true">smart_toy</span>{t("autoplay.title")}</h2>
            <nav className={styles.pageTabs} aria-label={t("autoplay.title")}>
                {(["strategy", "monitor", "connection"] as const).map(value => <button className="nav-btn" key={value} aria-pressed={tab === value}
                    onClick={() => setTab(value)}><span className="ms" aria-hidden="true">{{strategy: "tune", monitor: "monitoring", connection: "link"}[value]}</span>{t(value === "connection" ? "direct_connection.title" : `autoplay.tab_${value}`)}</button>)}
            </nav>
        </header>
        {tab === "strategy" ? <AutoPlayConfigPage/> : tab === "connection" ? <DirectConnectionPage/> : <AutoRunnerMonitorPage currentState={currentState}/>}
    </div>;
}

function AutoRunnerMonitorPage({currentState}: {currentState: GameStateData | null}) {
    const {t} = useTranslation();
    const {config, status} = useAutoRunner();
    const selectedProfile = config.playbook.profiles.find(profile => profile.id === config.playbook.selected_profile_id);
    const goalMet = status.current_step === "goal_met";
    const finished = goalMet || status.current_step === "completed" || status.current_step === "failed";
    const profile = status.running || finished ? status.active_profile : selectedProfile;
    const stepText = status.current_step?.startsWith("action:")
        ? t(`action_provider.actions.${status.current_step.slice(7)}`)
        : t(`autoplay.step_${status.current_step}`, {defaultValue: status.current_step ?? "—"});
    const openingProfile = profile?.strategy === "opening" ? profile : undefined;
    const openingCutoff = openingProfile && openingLevels.find(level => level.level === openingProfile.cutoff_level && level.node === openingProfile.cutoff_node);
    const achieved = status.running || goalMet ? status.current_achieved_count : undefined;
    const startUnavailable = !selectedProfile ? "autoplay.select_profile" : "";
    const {badgeById, amuletById} = useRegistry();

    const [saving, setSaving] = React.useState(false);
    const [settingsOpen, setSettingsOpen] = React.useState(false);
    const [saveNotice, setSaveNotice] = React.useState("");
    const [recordView, setRecordView] = React.useState<"remakes" | "operations">("remakes");
    const [selectedRecordSeq, setSelectedRecordSeq] = React.useState<number | null>(null);
    const [starting, setStarting] = React.useState(false);
    const [existingGameConfirm, setExistingGameConfirm] = React.useState(false);
    const [controlError, setControlError] = React.useState("");

    const working = Boolean(status.running);

    const [, forceTick] = React.useState(0);
    React.useEffect(() => {
        if (!status.running) return;
        const timer = setInterval(() => forceTick((x) => x + 1), 1000);
        return () => clearInterval(timer);
    }, [status.running]);

    const acceptControlResult = React.useCallback((data: backendIpc.CommandResult) => {
        setStarting(false);
        if (data.requires_confirmation) {
            setExistingGameConfirm(true);
            setControlError("");
            return;
        }
        setExistingGameConfirm(false);
        setControlError(data.ok ? "" : data.reason_key ? String(t(data.reason_key, data.reason_values ?? {})) : String(data.reason ?? ""));
    }, [t]);

    const onSave = React.useCallback(async () => {
        setSaving(true); setSaveNotice("");
        try {
            const {playbook: _playbook, ...monitorConfig} = config;
            await backendIpc.updateConfig({autorun: monitorConfig});
            setSaveNotice(t("autoplay.runtime_saved"));
        } catch {
            setSaveNotice(t("autoplay.save_failed"));
        } finally {
            setSaving(false);
        }
    }, [config, t]);

    const start = async (force = false) => {
        if (startUnavailable) {setControlError(t(startUnavailable)); return;}
        const invalid = validateAutoPlayConfig(config.playbook, amuletById, badgeById);
        if (invalid) {setControlError(t(`autoplay.error_${invalid}`)); return;}
        setStarting(true); setExistingGameConfirm(false); setControlError("");
        try {
            await backendIpc.updateConfig({autorun: config});
            markPlaybookSaved(config.playbook);
            acceptControlResult(await backendIpc.runAutorun("start", {force}));
        } catch {
            setControlError(t("autoplay.start_failed"));
        } finally { setStarting(false); }
    };

    const stop = React.useCallback(async () => {
        setExistingGameConfirm(false);
        setControlError("");
        acceptControlResult(await backendIpc.runAutorun("stop"));
    }, [acceptControlResult]);

    const cancelExistingGame = React.useCallback(() => {
        setStarting(false);
        setExistingGameConfirm(false);
        setControlError("");
    }, []);

    const elapsedDisplay = formatDuration(status.elapsed_ms ?? 0);

    const opInterval = Number.isFinite(Number(config.op_interval_ms)) ? Number(config.op_interval_ms) : 1000;
    const recordDetailedOperations = config.record_detailed_operations === true;
    const remakeRecords = React.useMemo(
        () => [...(status.remake_records ?? [])].sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0)),
        [status.remake_records],
    );
    const operationRecords = React.useMemo(
        () => [...(status.operation_records ?? [])].sort((a, b) => (b.seq ?? 0) - (a.seq ?? 0)).slice(0, 80),
        [status.operation_records],
    );
    const operationCounts = status.operation_count_by_run ?? {};
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
        if (reason === "custom-search-limit") return t("autorun.remake_reason_search_limit");
        if (reason === "invalid-search-tiles") return t("autorun.remake_reason_invalid_tiles");
        if (reason === "remake_at_cutoff") return t("autorun.remake_reason_cutoff");
        if (reason === "decision") return t("autorun.remake_reason_decision");
        return reason || "-";
    }, [t]);

    const describeRecord = React.useCallback((record: AutoRunnerRemakeRecord | null) => {
        if (!record) return "-";
        return t("autorun.remake_record_summary", {
            seq: record.seq ?? "-",
            run: record.run_index ?? "-",
            value: record.target_value ?? 0,
            count: record.amulet_count ?? 0,
            level: formatLevelIdToLabel(record.level, record.source, record.node ?? undefined),
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
                    const amulet = amuletById.get(regId);
                    const amuletName = (rawId % 10 === 1 ? amulet?.plus_name : amulet?.name) ?? amulet?.name ?? `ID ${regId || rawId}`;
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

    const operationDetailsText = React.useCallback((record: AutoRunnerOperationRecord) => {
        const details = record.details ?? {};
        if (!details || Object.keys(details).length === 0) return "";
        try {
            return JSON.stringify(details, null, 2);
        } catch {
            return String(details);
        }
    }, []);

    const candidateEffectItem = React.useCallback((item: Record<string, unknown>) => {
        const rawId = Number(item.raw_id ?? item.selected_raw_id ?? item.id ?? 0);
        const badgeId = Number(item.badge_id ?? item.selected_badge_id ?? item.badgeId ?? 0);
        return {
            id: rawId,
            volume: badgeId === 600160 ? 2 : 1,
            badge: badgeId > 0 ? {id: badgeId} : undefined,
        } as any;
    }, []);

    const candidateLabel = React.useCallback((item: Record<string, unknown>) => {
        const rawId = Number(item.raw_id ?? item.selected_raw_id ?? item.id ?? 0);
        const regId = Number(item.reg_id ?? (rawId > 0 ? Math.floor(rawId / 10) : 0));
        const amulet = amuletById.get(regId);
        const amuletName = (rawId % 10 === 1 ? amulet?.plus_name : amulet?.name) ?? amulet?.name ?? `ID ${regId || rawId || "-"}`;
        const badgeId = Number(item.badge_id ?? item.selected_badge_id ?? item.badgeId ?? 0);
        const badgeName = badgeId > 0 ? (badgeById.get(badgeId)?.name ?? String(badgeId)) : t("autorun.target_any_badge");
        return `${amuletName} / ${badgeName}`;
    }, [amuletById, badgeById, t]);

    const renderCandidateGrid = React.useCallback((items: unknown) => {
        const list = Array.isArray(items) ? items.filter((item): item is Record<string, unknown> => !!item && typeof item === "object") : [];
        if (list.length === 0) return null;
        return (
            <div
                style={{
                    display: "flex",
                    flexWrap: "nowrap",
                    gap: 10,
                    overflowX: "auto",
                    overflowY: "hidden",
                    padding: "6px 4px 10px",
                }}
            >
                {list.map((item, idx) => {
                    const selected = Boolean(item.selected);
                    return (
                        <div
                            key={`${Number(item.raw_id ?? item.id ?? idx)}-${idx}`}
                            style={{
                                display: "grid",
                                gap: 6,
                                justifyItems: "center",
                                padding: 8,
                                border: `1px solid ${selected ? "var(--accent-green)" : "var(--border)"}`,
                                borderRadius: 8,
                                background: selected ? "color-mix(in srgb, var(--accent-green) 10%, transparent)" : "transparent",
                                width: 142,
                                flex: "0 0 auto",
                            }}
                        >
                            <AmuletCard item={candidateEffectItem(item)} scale={0.62} showPrice/>
                            <div style={{fontWeight: 700, fontSize: 12, textAlign: "center", width: "100%"}}>{candidateLabel(item)}</div>
                            <div className="hint" style={{fontSize: 12, display: "grid", gap: 2, width: "100%", textAlign: "left"}}>
                                <span>{selected ? t("autorun.operation_candidate_selected") : t("autorun.operation_candidate_skipped")}</span>
                                {"selection_value" in item ? <span>{t("autorun.operation_candidate_value", {value: String(item.selection_value ?? "-")})}</span> : null}
                                {"price" in item ? <span>{t("autorun.operation_candidate_price", {price: String(item.price ?? "-")})}</span> : null}
                                {"value_reason" in item ? <span title={String(item.value_reason ?? "")}>{t("autorun.operation_value_reason", {reason: String(item.value_reason ?? "-")})}</span> : null}
                                {"reason" in item ? <span title={String(item.reason ?? "")}>{t("autorun.operation_reason_label", {reason: String(item.reason ?? "-")})}</span> : null}
                            </div>
                        </div>
                    );
                })}
            </div>
        );
    }, [candidateEffectItem, candidateLabel, t]);

    const renderOperationDetails = React.useCallback((record: AutoRunnerOperationRecord) => {
        const details = (record.details ?? {}) as Record<string, unknown>;
        const considered = renderCandidateGrid(details.considered);
        const decision = details.decision && typeof details.decision === "object" ? details.decision as Record<string, unknown> : null;
        const decisionCandidates = decision ? renderCandidateGrid(decision.considered) : null;
        const selectedRawId = details.selected_raw_id ?? decision?.raw_id;
        const selectedBadgeId = details.selected_badge_id ?? decision?.badge_id;
        const newEffect = details.new_effect && typeof details.new_effect === "object" ? details.new_effect as Record<string, unknown> : null;
        const sellItem = details.sell_item && typeof details.sell_item === "object" ? details.sell_item as Record<string, unknown> : null;
        const detailsText = operationDetailsText(record);

        return (
            <div className="hint" style={{display: "grid", gap: 8, marginTop: 8, lineHeight: 1.45}}>
                <div>{t("autorun.operation_reason_label", {reason: record.reason || "-"})}</div>
                <div>{t("autorun.operation_step_label", {step: record.step || "-", level: formatLevelIdToLabel(record.level)})}</div>
                <div>{t("autorun.operation_time_label", {time: record.ts ? new Date(record.ts).toLocaleString() : "-"})}</div>
                <div style={{display: "flex", gap: 8, flexWrap: "wrap"}}>
                    {selectedRawId != null ? <span className="badge">{t("autorun.operation_selected_raw", {id: String(selectedRawId)})}</span> : null}
                    {selectedBadgeId != null ? <span className="badge">{t("autorun.operation_selected_badge", {id: String(selectedBadgeId)})}</span> : null}
                    {details.selection_value != null ? <span className="badge">{t("autorun.operation_selection_value", {value: String(details.selection_value)})}</span> : null}
                    {details.sell_uid != null ? <span className="badge">{t("autorun.operation_sell_uid", {uid: String(details.sell_uid)})}</span> : null}
                    {details.need_space != null ? <span className="badge">{t("autorun.operation_need_space", {value: String(details.need_space)})}</span> : null}
                    {details.free_space != null ? <span className="badge">{t("autorun.operation_free_space", {value: String(details.free_space)})}</span> : null}
                </div>
                {newEffect ? (
                    <div>
                        <div style={{fontWeight: 700, marginBottom: 6}}>{t("autorun.operation_new_effect")}</div>
                        {renderCandidateGrid([newEffect])}
                    </div>
                ) : null}
                {sellItem ? (
                    <div>
                        <div style={{fontWeight: 700, marginBottom: 6}}>{t("autorun.operation_sell_item")}</div>
                        {renderCandidateGrid([sellItem])}
                    </div>
                ) : null}
                {considered ? (
                    <div>
                        <div style={{fontWeight: 700, marginBottom: 6}}>{t("autorun.operation_candidates_title")}</div>
                        {considered}
                    </div>
                ) : null}
                {!considered && decisionCandidates ? (
                    <div>
                        <div style={{fontWeight: 700, marginBottom: 6}}>{t("autorun.operation_candidates_title")}</div>
                        {decisionCandidates}
                    </div>
                ) : null}
                {detailsText ? (
                    <details>
                        <summary style={{cursor: "pointer"}}>{t("autorun.operation_raw_details")}</summary>
                        <pre style={{margin: "8px 0 0", whiteSpace: "pre-wrap", wordBreak: "break-word", fontSize: 12}}>
                            {detailsText}
                        </pre>
                    </details>
                ) : null}
            </div>
        );
    }, [operationDetailsText, renderCandidateGrid, t]);

    const renderOperationRecords = React.useCallback((records: AutoRunnerOperationRecord[]) => {
        if (!records || records.length === 0) {
            return <div className="hint">{t("autorun.operation_empty")}</div>;
        }
        return (
            <div style={{display: "grid", gap: 8, maxHeight: 420, overflowY: "auto", paddingRight: 4}}>
                {records.map((record) => {
                    return (
                        <details key={record.seq} style={{padding: 10, border: "1px solid var(--border)", borderRadius: 8}}>
                            <summary style={{cursor: "pointer"}}>
                                <span className="badge" style={{marginRight: 8}}>#{record.seq}</span>
                                <span>
                                    {t("autorun.operation_record_row", {
                                        run: record.run_index ?? "-",
                                        op: record.op_index ?? "-",
                                        action: record.action || "-",
                                        result: record.result || "-",
                                    })}
                                </span>
                            </summary>
                            {renderOperationDetails(record)}
                        </details>
                    );
                })}
            </div>
        );
    }, [renderOperationDetails, t]);

    return <div className={`${styles.page} ${styles.monitor}`}>
        <section className={`panel ${styles.runOverview}`}>
            <div className={styles.runHeading}>
                <div className={styles.runState}>
                    <span className={`${styles.stateIcon} ${working ? styles.running : ""}`}><span className="ms" aria-hidden="true">{working ? "play_arrow" : "pause"}</span></span>
                    <div><h3>{finished ? stepText : t(working ? "autorun.status_running" : "autorun.status_stopped")}</h3>
                        <p>{t("autorun.hint_current_step", {text: stepText})}</p></div>
                </div>
                <div className={styles.toolbar}>
                    <button className="nav-btn" onClick={() => {setSaveNotice(""); setSettingsOpen(true);}}><span className="ms" aria-hidden="true">tune</span>{t("autoplay.runtime_settings")}</button>
                    <button className={`nav-btn ${styles.primary}`} onClick={() => working ? void stop() : void start()} disabled={starting || existingGameConfirm || (!working && !!startUnavailable)} title={!working && startUnavailable ? t(startUnavailable) : undefined}>
                        <span className="ms" aria-hidden="true">{working ? "pause" : "play_arrow"}</span>{t(working ? "autorun.btn_pause" : starting ? "autorun.btn_starting" : "autorun.btn_start")}</button>
                </div>
            </div>
            {existingGameConfirm && <div className={styles.confirmation} role="alert">
                <span>{t("autorun.confirm_existing_game")}</span><div className={styles.toolbar}>
                    <button className="nav-btn" onClick={() => void start(true)} disabled={starting}><span className="ms" aria-hidden="true">play_arrow</span>{t("autorun.btn_continue_game")}</button>
                    <button className="nav-btn" onClick={cancelExistingGame} disabled={starting}><span className="ms" aria-hidden="true">close</span>{t("autorun.btn_cancel")}</button>
                </div>
            </div>}
            {controlError && <p className="notice error" role="alert">{controlError}</p>}
            {status.last_error && <p className="notice error">{t("autorun.hint_last_error", {text: t(status.last_error, {defaultValue: status.last_error})})}</p>}
            {working && <p className="hint">{t("autoplay.fuse_suspended")}</p>}
            {status.email_status && <p className={status.email_status === "failed" ? "notice error" : "hint"} role="status">
                <span className="ms" aria-hidden="true">{status.email_status === "failed" ? "error" : "mail"}</span>{t(`autoplay.email_${status.email_status}`, {error: status.email_error ?? ""})}
            </p>}
            <div className={styles.metrics}>
                <div><span><span className="ms" aria-hidden="true">timer</span>{t("autoplay.elapsed")}</span><strong>{elapsedDisplay}</strong></div>
                <div><span><span className="ms" aria-hidden="true">replay</span>{t("autoplay.runs")}</span><strong>{status.runs ?? 0}</strong></div>
                <div><span><span className="ms" aria-hidden="true">workspace_premium</span>{t("autoplay.best_value")}</span><strong>{status.best_achieved_count ?? 0}</strong></div>
                <div><span><span className="ms" aria-hidden="true">speed</span>{t("autorun.label_op_interval")}</span><strong>{opInterval}<small>{t("autorun.suffix_ms")}</small></strong></div>
            </div>
            <div className={styles.runMeta}><span>{t("autorun.hint_started_at", {time: status.started_at ? new Date(status.started_at).toLocaleString() : "-"})}</span>
                {profile && <span>{t("autoplay.running_profile", {name: profile.name})}</span>}</div>
        </section>

        <RoundDecisionPanel decision={status.active_profile?.id === profile?.id ? status.round_decision : undefined} running={working} profile={profile}/>
        <ActionProviderPanel state={currentState}/>

        {openingProfile ? <section className={`panel ${styles.progressPanel}`}>
            <div className={styles.goalProgress}>
                <h3 className="panel-title">{openingProfile.name}</h3>
                <div className={styles.progressValue}><strong>{achieved ?? "—"}</strong><span>/ {openingProfile.end_count}</span></div>
                <progress max={openingProfile.end_count} value={achieved == null ? undefined : Math.max(0, achieved)} aria-label={t("autoplay.goal_progress")}/>
                <span className="hint">{openingCutoff ? t("autoplay.cutoff_at", {level: openingCutoff.label}) : t("autoplay.error_cutoff")}</span>
            </div>
            <div className={styles.currentTargets}><AutoPlayTargets targets={openingProfile.targets}/></div>
        </section> : <section className="panel">
            <h3 className="panel-title">{profile?.name ?? t("autoplay.profile")}</h3>
            {profile && profile.strategy !== "opening" && <div className={styles.toolbar}>{profile.stages.map(stage => <span className="badge" key={stage.id}>
                {stage.name} · {t("autoplay.cutoff_at", {level: formatLevelIdToLabel(stage.deadline_level, "packet", stage.deadline_node)})}
            </span>)}</div>}
            <p className="hint">{t(profile ? "round.stage_hint" : "autoplay.select_profile")}</p>
        </section>}

        <section className={`panel ${styles.historyPanel}`}>
            <div className={styles.historyHeader}>
                <div className={styles.choices} role="group" aria-label={t("autoplay.records")}>
                    <button className="nav-btn" aria-pressed={recordView === "remakes"} onClick={() => setRecordView("remakes")}><span className="ms" aria-hidden="true">history</span>{t("autorun.section_remake_records_title")}<span className="badge">{remakeRecords.length}</span></button>
                    <button className="nav-btn" aria-pressed={recordView === "operations"} onClick={() => setRecordView("operations")}><span className="ms" aria-hidden="true">list_alt</span>{t("autorun.section_operation_records_title")}<span className="badge">{operationRecords.length}</span></button>
                </div>
                {recordView === "remakes" && bestRecord && <button className="nav-btn" onClick={() => setSelectedRecordSeq(bestRecord.seq)}><span className="ms" aria-hidden="true">workspace_premium</span>{t("autoplay.view_best")}</button>}
            </div>
            {recordView === "remakes" && <p className="hint">{t("autorun.remake_history_hint")}</p>}
            {recordView === "remakes" ? remakeRecords.length === 0 ? <div className={styles.empty}><span className="ms" aria-hidden="true">history</span><p>{t("autorun.remake_empty")}</p></div> :
                <div className={styles.recordLayout}>
                    <div className={styles.recordList} role="group" aria-label={t("autorun.section_remake_records_title")}>
                        {remakeRecords.map(record => <button key={record.seq} className={styles.recordButton} aria-pressed={selectedRecord?.seq === record.seq}
                            onClick={() => setSelectedRecordSeq(record.seq)} title={reasonText(record.reason)}>
                            <span className="ms" aria-hidden="true">{bestRecord?.seq === record.seq ? "workspace_premium" : "replay"}</span>
                            <span><strong>#{record.seq} · {t("autoplay.run_number", {run: record.run_index})}</strong><small>{[record.profile_name, formatLevelIdToLabel(record.level, record.source, record.node ?? undefined), reasonText(record.reason)].filter(Boolean).join(" · ")}</small></span>
                            <span className={styles.recordValue}><b>{record.target_value}</b><small>{t(bestRecord?.seq === record.seq ? "autorun.remake_best_marker" : "autoplay.value")}</small></span>
                        </button>)}
                    </div>
                    <div className={styles.recordDetail}>
                        <div className={styles.recordDetailHeading}><strong>{describeRecord(selectedRecord)}</strong><span className="hint">{selectedRecord?.ts ? new Date(selectedRecord.ts).toLocaleString() : "—"}</span></div>
                        <p className="hint">{t("autorun.remake_reason_label", {reason: reasonText(selectedRecord?.reason)})}</p>
                        <div className={styles.recordAmulets}>{renderRecordAmulets(selectedRecord)}</div>
                        {recordDetailedOperations && <details className={styles.recordOperations}><summary>{t("autorun.remake_operation_count", {count: selectedRecord?.operation_count ?? selectedRecord?.operation_records?.length ?? 0})}</summary>
                            {renderOperationRecords(selectedRecord?.operation_records ?? [])}</details>}
                    </div>
                </div> : <div className={styles.operationPanel}>
                    <div className={styles.toolbar}><label className={styles.check}><input type="checkbox" checked={recordDetailedOperations} onChange={event => patchAutoConfig({record_detailed_operations: event.target.checked})}/>{t("autorun.toggle_record_detailed_operations")}</label>
                        {Object.entries(operationCounts).sort(([a], [b]) => Number(b) - Number(a)).slice(0, 6).map(([run, count]) => <span className="badge" key={run}>{t("autorun.operation_count_by_run", {run, count})}</span>)}
                    </div>
                    {renderOperationRecords(operationRecords)}
                </div>}
        </section>

        <Modal open={settingsOpen} onClose={() => setSettingsOpen(false)} title={t("autoplay.runtime_settings")} width={720} className={`${styles.dialog} ${styles.page}`}
            actions={<button className={`nav-btn ${styles.primary}`} onClick={onSave} disabled={saving}><span className="ms" aria-hidden="true">save</span>{t(saving ? "autorun.btn_saving" : "autorun.btn_save")}</button>}>
            <fieldset className={styles.content} disabled={saving}>
                <section className="panel"><h3 className="panel-title">{t("autoplay.runtime_options")}</h3>
                    <div className={styles.openingFields}>
                        <label className={`${styles.field} ${styles.valueField}`}>{t("autorun.label_op_interval")} ({t("autorun.suffix_ms")})<input className="form-input" type="number" min={0} max={5000} value={opInterval}
                            title={t("autorun.tip_op_interval")} onChange={event => patchAutoConfig({op_interval_ms: Math.max(0, Math.min(5000, Math.round(Number(event.target.value || 0))))})}/></label>

                    </div>
                    <label className={styles.check}><input type="checkbox" checked={recordDetailedOperations} onChange={event => patchAutoConfig({record_detailed_operations: event.target.checked})}/>{t("autorun.toggle_record_detailed_operations")}</label>
                </section>
                <details className={`panel ${styles.emailSettings}`}><summary><span className="ms" aria-hidden="true">mail</span>{t("autorun.section_email_title")}</summary>
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

                                <div className={styles.emailFields}>
                                    <label className={styles.field}>
                                        <span>{t("autorun.label_smtp_host")}</span>
                                        <input className="form-input" placeholder="smtp.example.com" value={email.host ?? ""} onChange={(e) => patchEmail({host: e.target.value.trim()})}/>
                                    </label>

                                    <label className={styles.field}>
                                        <span>{t("autorun.label_smtp_port")}</span>
                                        <input className="form-input" type="number" min={1} max={65535} value={Number(email.port ?? 587)} onChange={(e) => patchEmail({port: Math.max(1, Math.min(65535, Number(e.target.value || 587)))})}/>
                                    </label>

                                    <label className={styles.field}>
                                        <span>{t("autorun.label_smtp_ssl")}</span>
                                        <input className="form-checkbox" type="checkbox" checked={!!email.ssl} onChange={(e) => patchEmail({ssl: e.target.checked})}/>
                                    </label>
                                </div>

                                <div className={styles.emailFields}>
                                    <label className={styles.field}>
                                        <span>{t("autorun.label_email_from")}</span>
                                        <input className="form-input" type="email" placeholder="sender@example.com" value={email.from ?? ""} onChange={(e) => patchEmail({from: e.target.value.trim()})}/>
                                    </label>

                                    <label className={styles.field}>
                                        <span>{t("autorun.label_email_pass")}</span>
                                        <input className="form-input" type="password" placeholder={t("autorun.label_email_pass_placeholder")} value={email.pass ?? ""} onChange={(e) => patchEmail({pass: e.target.value})}/>
                                    </label>
                                </div>

                                <div className={styles.emailFields}>
                                    <label className={styles.field}>
                                        <span>{t("autorun.label_email_to")}</span>
                                        <input className="form-input" type="email" placeholder="you@example.com" value={email.to ?? ""} onChange={(e) => patchEmail({to: e.target.value.trim()})}/>
                                    </label>
                                </div>

                                <div className="toolbar" style={{gap: 8, marginTop: 6, flexWrap: "wrap" as const}}>
                                    <button
                                        className="nav-btn"
                                        onClick={() => void backendIpc.runAutorun("notify_test_email")}
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
                                        <span className="ms" aria-hidden="true">send</span>{t("autorun.btn_send_test")}
                                    </button>
                                </div>
                            </div>
                        );
                    })()}
                </details>
                <div role="status">{saveNotice}</div>
            </fieldset>
        </Modal>

    </div>;
}
