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
    type AutoRunnerConfig,
    type AutoRunnerOperationRecord,
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
import {formatLevelIdToLabel} from "../lib/levelFormat";

function formatDuration(ms: number): string {
    if (!ms || ms < 0) ms = 0;
    const s = Math.floor(ms / 1000);
    const hh = Math.floor(s / 3600);
    const mm = Math.floor((s % 3600) / 60);
    const ss = s % 60;
    const pad = (n: number) => n.toString().padStart(2, "0");
    return `${pad(hh)}:${pad(mm)}:${pad(ss)}`;
}

const DENSE_ALPHABET = Array.from({length: 94}, (_, i) => String.fromCharCode(i + 33))
    .filter((ch) => ch !== "\\" && ch !== "`" && ch !== "\"")
    .join("");
const PIONNER_BADGE_COUNT_SENTINEL = 990000;

function toDenseText(text: string): string {
    if (!text) return "!";
    const bytes = new TextEncoder().encode(text);
    let value = 0n;
    for (const byte of bytes) {
        value = (value << 8n) + BigInt(byte);
    }
    const base = BigInt(DENSE_ALPHABET.length);
    let out = "";
    while (value > 0n) {
        out = DENSE_ALPHABET[Number(value % base)] + out;
        value /= base;
    }
    return out;
}

function fromDenseText(text: string): string {
    if (text === "!") return "";
    if (!text) return "";
    const base = BigInt(DENSE_ALPHABET.length);
    let value = 0n;
    for (const ch of text) {
        const digit = DENSE_ALPHABET.indexOf(ch);
        if (digit < 0) throw new Error("bad_dense_char");
        value = value * base + BigInt(digit);
    }
    const bytes: number[] = [];
    while (value > 0n) {
        bytes.unshift(Number(value & 0xffn));
        value >>= 8n;
    }
    return new TextDecoder().decode(new Uint8Array(bytes));
}

function encodeShortConfig(config: AutoRunnerConfig): string {
    const parts: string[] = [];
    const endCount = Math.max(1, Math.floor(Number(config.end_count ?? 1)));
    const cutoffLevel = Number(config.cutoff_level ?? 0) || 0;
    const interval = Math.max(0, Math.round(Number(config.op_interval_ms ?? 1000)));
    const needPionnerBadgeCount = Math.max(0, Math.floor(Number(config.need_pionner_badge_count ?? 4)));
    if (endCount !== 1) parts.push(`e${endCount}`);
    if (needPionnerBadgeCount !== 4) {
        parts.push(`l${PIONNER_BADGE_COUNT_SENTINEL + Math.min(99, needPionnerBadgeCount)}`);
    }
    if (cutoffLevel > 0) parts.push(`l${cutoffLevel}`);
    if (needPionnerBadgeCount !== 4 && cutoffLevel <= 0) parts.push("l0");
    if (interval !== 1000) parts.push(`i${interval}`);
    parts.push(...(config.targets ?? []).map((target) => {
        const value = Math.max(1, Math.floor(Number((target as any).value ?? 1)));
        const valueSuffix = value === 1 ? "" : `:${value}`;
        if (target.kind === "amulet") {
            const plus = target.plus ? "+" : "";
            const badge = target.badge == null ? "" : `.${target.badge}`;
            return `a${target.id}${plus}${badge}${valueSuffix}`;
        }
        return `b${target.id}${valueSuffix}`;
    }));
    return toDenseText(parts.join(";"));
}

function decodeShortConfig(text: string): Partial<AutoRunnerConfig> {
    const raw = text.trim();
    return decodeCompactConfigText(fromDenseText(raw));
}

function decodeCompactConfigText(payloadText: string): Partial<AutoRunnerConfig> {
    const targets: TargetItem[] = [];
    let endCount = 1;
    let cutoffLevel = 0;
    let interval = 1000;
    let needPionnerBadgeCount = 4;

    for (const part of payloadText.split(";").filter(Boolean)) {
        if (part.startsWith("e")) {
            endCount = Math.max(1, Math.floor(Number(part.slice(1))));
            continue;
        }
        if (part.startsWith("l")) {
            const level = Math.max(0, Math.floor(Number(part.slice(1))));
            if (level >= PIONNER_BADGE_COUNT_SENTINEL && level < PIONNER_BADGE_COUNT_SENTINEL + 100) {
                needPionnerBadgeCount = level - PIONNER_BADGE_COUNT_SENTINEL;
                continue;
            }
            cutoffLevel = level;
            continue;
        }
        if (part.startsWith("i")) {
            interval = Math.max(0, Math.min(5000, Math.round(Number(part.slice(1)))));
            continue;
        }
        if (part.startsWith("p")) {
            needPionnerBadgeCount = Math.max(0, Math.floor(Number(part.slice(1))));
            continue;
        }
        const valueSplit = part.split(":");
        const head = valueSplit[0];
        const value = Math.max(1, Math.floor(Number(valueSplit[1] ?? 1)));
        if (head.startsWith("b")) {
            const id = Number(head.slice(1));
            if (!Number.isFinite(id)) throw new Error("bad_target");
            targets.push({kind: "badge", id, value});
            continue;
        }
        if (head.startsWith("a")) {
            const match = /^a(\d+)(\+?)(?:\.(\d+))?$/.exec(head);
            if (!match) throw new Error("bad_target");
            targets.push({
                kind: "amulet",
                id: Number(match[1]),
                plus: match[2] === "+",
                badge: match[3] == null ? null : Number(match[3]),
                value,
            });
            continue;
        }
        throw new Error("bad_part");
    }

    return {
        end_count: endCount,
        cutoff_level: cutoffLevel,
        op_interval_ms: interval,
        need_pionner_badge_count: needPionnerBadgeCount,
        targets,
    };
}

export default function AutoRunnerPage() {
    const {t} = useTranslation();
    const {config, status} = useAutoRunner();
    const {badgeById, amuletById} = useRegistry();

    const [saving, setSaving] = React.useState(false);
    const [openAmuletEditor, setOpenAmuletEditor] = React.useState(false);
    const [openBadgePicker, setOpenBadgePicker] = React.useState(false);
    const [detailTargetIndex, setDetailTargetIndex] = React.useState<number | null>(null);
    const [selectedRecordSeq, setSelectedRecordSeq] = React.useState<number | null>(null);
    const [levelText, setLevelText] = React.useState<string>(formatLevelNum(config.cutoff_level));
    const [exportOpen, setExportOpen] = React.useState(false);
    const [exportText, setExportText] = React.useState("");
    const [importOpen, setImportOpen] = React.useState(false);
    const [importText, setImportText] = React.useState("");
    const [importError, setImportError] = React.useState("");
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

    React.useEffect(() => {
        setLevelText(formatLevelNum(config.cutoff_level));
    }, [config.cutoff_level]);

    React.useEffect(() => {
        const off = ws.onPacket((pkt) => {
            if (pkt.type !== "autorun_control_result") return;
            const data = pkt.data ?? {};
            setStarting(false);
            if (data.requires_confirmation) {
                setExistingGameConfirm(true);
                setControlError("");
                return;
            }
            setExistingGameConfirm(false);
            setControlError(data.ok ? "" : data.reason_key ? String(t(data.reason_key, data.reason_values ?? {})) : String(data.reason ?? ""));
        });
        return off;
    }, [t]);

    const onSave = React.useCallback(() => {
        setSaving(true);
        try {
            ws.send({type: "edit_config", data: {autorun: config}});
        } finally {
            setSaving(false);
        }
    }, [config]);

    const start = React.useCallback(() => {
        setStarting(true);
        setExistingGameConfirm(false);
        setControlError("");
        ws.send({type: "autorun_control", data: {action: "start"}});
    }, []);

    const stop = React.useCallback(() => {
        setExistingGameConfirm(false);
        setControlError("");
        ws.send({type: "autorun_control", data: {action: "stop"}});
    }, []);

    const continueExistingGame = React.useCallback(() => {
        setStarting(true);
        setExistingGameConfirm(false);
        setControlError("");
        ws.send({type: "autorun_control", data: {action: "start", force: true}});
    }, []);

    const cancelExistingGame = React.useCallback(() => {
        setStarting(false);
        setExistingGameConfirm(false);
        setControlError("");
    }, []);

    const openExport = React.useCallback(async () => {
        const text = encodeShortConfig(config);
        setExportText(text);
        setExportOpen(true);
        try {
            await navigator.clipboard.writeText(text);
        } catch {
            // Clipboard access can be unavailable; the modal still shows the code.
        }
    }, [config]);

    const applyImport = React.useCallback(() => {
        try {
            const next = decodeShortConfig(importText);
            patchAutoConfig(next);
            setLevelText(formatLevelNum(next.cutoff_level));
            setImportOpen(false);
            setImportText("");
            setImportError("");
        } catch {
            setImportError(t("autorun.import_error"));
            return;
        }
    }, [importText, t]);

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
        return reason || "-";
    }, [t]);

    const describeRecord = React.useCallback((record: AutoRunnerRemakeRecord | null) => {
        if (!record) return "-";
        return t("autorun.remake_record_summary", {
            seq: record.seq ?? "-",
            run: record.run_index ?? "-",
            value: record.target_value ?? 0,
            count: record.amulet_count ?? 0,
            level: formatLevelIdToLabel(record.level),
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
        const amuletName = amuletById.get(regId)?.name ?? `ID ${regId || rawId || "-"}`;
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
                            {existingGameConfirm ? (
                                <div style={{display: "inline-flex", alignItems: "center", gap: 8, flexWrap: "wrap"}}>
                                    <span className="hint">{t("autorun.confirm_existing_game")}</span>
                                    <button className="nav-btn" onClick={continueExistingGame} disabled={starting}>
                                        {starting ? t("autorun.btn_starting") : t("autorun.btn_continue_remake")}
                                    </button>
                                    <button className="nav-btn" onClick={cancelExistingGame} disabled={starting}>
                                        {t("autorun.btn_cancel")}
                                    </button>
                                </div>
                            ) : (
                                <button
                                    className="nav-btn"
                                    onClick={working ? stop : start}
                                    disabled={starting || status.mode === "step"}
                                    title={status.mode === "step" ? t("autorun.tip_no_need_start_in_step") : undefined}
                                >
                                    {working ? t("autorun.btn_pause") : starting ? t("autorun.btn_starting") : t("autorun.btn_start")}
                                </button>
                            )}

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
                        </div>

                        <p className="hint" style={{marginTop: 8, lineHeight: 1.5}}>
                            {t("autorun.control_note")}
                        </p>
                        {controlError ? <p className="notice error" style={{marginTop: 8}}>{controlError}</p> : null}
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
                                                    level: formatLevelIdToLabel(record.level),
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
                                    {recordDetailedOperations ? (
                                        <span className="badge">
                                            {t("autorun.remake_operation_count", {count: selectedRecord?.operation_count ?? selectedRecord?.operation_records?.length ?? 0})}
                                        </span>
                                    ) : null}
                                </div>
                                <div style={{maxHeight: 360, overflowY: "auto", paddingRight: 4}}>
                                    {renderRecordAmulets(selectedRecord)}
                                </div>
                                {recordDetailedOperations ? (
                                    <div style={{display: "grid", gap: 8}}>
                                        <div className="panel-title" style={{fontSize: 14}}>{t("autorun.remake_operation_records_title")}</div>
                                        {renderOperationRecords(selectedRecord?.operation_records ?? [])}
                                    </div>
                                ) : null}
                            </div>
                        </div>
                    )}
                </section>

                {recordDetailedOperations ? (
                    <section className="panel">
                        <div className="panel-title">{t("autorun.section_operation_records_title")}</div>
                        <div style={{display: "flex", gap: 12, flexWrap: "wrap", marginBottom: 10}}>
                            <span className="badge">{t("autorun.operation_record_count", {count: operationRecords.length})}</span>
                            {Object.entries(operationCounts)
                                .sort(([a], [b]) => Number(b) - Number(a))
                                .slice(0, 6)
                                .map(([run, count]) => (
                                    <span className="badge" key={run}>
                                        {t("autorun.operation_count_by_run", {run, count})}
                                    </span>
                                ))}
                        </div>
                        {operationRecords.length === 0 ? (
                            <div className="hint">{t("autorun.operation_empty")}</div>
                        ) : (
                            renderOperationRecords(operationRecords)
                        )}
                    </section>
                ) : null}

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
                            <div className="row flush" style={{gridTemplateColumns: "max-content max-content", alignItems: "center"}}>
                                <label>{t("autorun.label_need_pionner_badge_count")}</label>
                                <input
                                    className="form-input"
                                    type="number"
                                    min={0}
                                    max={99}
                                    value={Number(config.need_pionner_badge_count ?? 4)}
                                    onChange={(e) => {
                                        const value = Math.max(0, Math.min(99, Math.floor(Number(e.target.value || 0))));
                                        patchAutoConfig({need_pionner_badge_count: value});
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

                            <label style={{display: "inline-flex", alignItems: "center", gap: 8, whiteSpace: "nowrap"}}>
                                <input
                                    type="checkbox"
                                    checked={recordDetailedOperations}
                                    onChange={(e) => patchAutoConfig({record_detailed_operations: e.target.checked})}
                                />
                                <span>{t("autorun.toggle_record_detailed_operations")}</span>
                            </label>
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
                    <button className="nav-btn" onClick={openExport}>
                        {t("autorun.btn_export_config")}
                    </button>
                    <button className="nav-btn" onClick={() => {
                        setImportOpen(true);
                        setImportText("");
                        setImportError("");
                    }}>
                        {t("autorun.btn_import_config")}
                    </button>
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
            <Modal open={exportOpen} onClose={() => {
                setExportOpen(false);
                setExportText("");
            }} title={t("autorun.export_title")} width={640}>
                <div style={{display: "grid", gap: 10}}>
                    <textarea className="form-input" value={exportText} readOnly rows={4} style={{fontFamily: "monospace", resize: "vertical"}}/>
                    <div className="hint">{t("autorun.export_hint")}</div>
                </div>
            </Modal>
            <Modal open={importOpen} onClose={() => {
                setImportOpen(false);
                setImportText("");
                setImportError("");
            }} title={t("autorun.import_title")} width={640}>
                <div style={{display: "grid", gap: 10}}>
                    <textarea
                        className="form-input"
                        value={importText}
                        onChange={(e) => {
                            setImportText(e.target.value);
                            setImportError("");
                        }}
                        rows={4}
                        placeholder={t("autorun.import_placeholder")}
                        style={{fontFamily: "monospace", resize: "vertical"}}
                    />
                    {importError ? <div className="notice error">{importError}</div> : <div className="hint">{t("autorun.import_hint")}</div>}
                    <div className="toolbar" style={{justifyContent: "flex-end", gap: 8}}>
                        <button className="nav-btn" onClick={applyImport} disabled={!importText.trim()}>
                            {t("autorun.btn_apply_import")}
                        </button>
                    </div>
                </div>
            </Modal>
        </div>
    );
}
