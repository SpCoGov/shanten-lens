import React from "react";
import {useTranslation} from "react-i18next";
import type {ActionCandidate, ActionReason, Availability, JsonValue} from "../bindings";
import type {GameStateData} from "../lib/gamestate";
import {useRegistry} from "../lib/registryStore";
import {formatLevelIdToLabel} from "../lib/levelFormat";
import {executeAction} from "../lib/ipc";
import {actionScoreTotal, compareActionScores} from "../lib/actionScores";
import Modal from "./Modal";
import styles from "./ActionProviderPanel.module.css";

function object(value: JsonValue | undefined): Record<string, JsonValue | undefined> {
    return value && typeof value === "object" && !Array.isArray(value) ? value : {};
}

const icons: Record<string, string> = {
    select_character: "person", select_candidate: "style", buy_pack: "shopping_bag", buy_effect: "shopping_cart",
    buy_rune: "diamond", sell_effect: "sell", shop_upgrade: "upgrade", advanced_upgrade: "upgrade",
    refresh_shop: "refresh", next_node: "arrow_forward", skip: "skip_next", transport: "travel_explore",
    trade: "swap_horiz", forge_effect: "hardware", bonfire: "local_fire_department", gamble: "casino",
    start_game: "play_arrow", remake: "restart_alt",
};

export default function ActionProviderPanel({state}: {state: GameStateData | null}) {
    const {t, i18n} = useTranslation();
    const {amuletById, badgeById} = useRegistry();
    const [filter, setFilter] = React.useState<"all" | Availability>("all");
    const [detailsOpen, setDetailsOpen] = React.useState(false);
    const detailsButton = React.useRef<HTMLButtonElement>(null);
    const closeDetails = React.useCallback(() => {
        setDetailsOpen(false);
        detailsButton.current?.focus();
    }, []);
    const executing = React.useRef(false);
    const [pending, setPending] = React.useState<string | null>(null);
    const [submitted, setSubmitted] = React.useState<string | null>(null);
    const [feedback, setFeedback] = React.useState<{error: boolean; text: string} | null>(null);
    const stamp = `${state?.session_id}:${state?.revision}`;
    const waiting = submitted === stamp;
    const execute = async (action: ActionCandidate) => {
        if (executing.current || waiting || action.availability !== "available" || state?.session_id == null || state.revision == null) return;
        executing.current = true;
        setPending(action.id);
        setFeedback(null);
        try {
            const result = await executeAction({action_id: action.id, session_id: state.session_id, revision: state.revision});
            if (result.ok || result.msg_id != null || result.reason_key === "action_provider.execution.uncertain" || result.reason_key === "action_provider.execution.awaiting_state") setSubmitted(stamp);
            setFeedback({error: !result.ok, text: result.ok
                ? t("action_provider.execution.success", {action: t(`action_provider.actions.${action.kind}`)})
                : t(result.reason_key || "action_provider.execution.send_failed", {...result.reason_values})});
        } catch {
            setSubmitted(stamp);
            setFeedback({error: true, text: t("action_provider.execution.uncertain")});
        } finally {
            executing.current = false;
            setPending(null);
        }
    };
    const report = state?.action_report;
    const scores = new Map((state?.decision_report?.actions ?? []).map(score => [score.action_id, score]));
    const byScore = (a: ActionCandidate, b: ActionCandidate) => compareActionScores(a, b, scores);
    const actions = [...(report?.actions ?? [])].sort(byScore);
    const globalActions = [...(report?.global_actions ?? [])].sort(byScore);
    const counts = {all: actions.length, available: 0, unavailable: 0, unknown: 0};
    for (const action of actions) counts[action.availability]++;
    const visible = actions.filter(action => filter === "all" || action.availability === filter);
    const scoredActions = [...actions, ...globalActions].filter(action => action.availability === "available").sort(byScore);
    const signed = (value: number) => value.toLocaleString(i18n.resolvedLanguage, {maximumFractionDigits: 2, useGrouping: false, signDisplay: "exceptZero"});
    const scoreTone = (value: number) => value > 0 ? styles.positive : value < 0 ? styles.negative : "";
    const scoreBadge = (action: ActionCandidate) => {
        const total = actionScoreTotal(scores.get(action.id));
        return <span className={`${styles.score} ${scoreTone(total)}`}>
            <span className={styles.scoreLabel}>{t("action_provider.scoring.label")}</span>
            <strong className={styles.scoreValue}>{signed(total)}</strong>
        </span>;
    };
    const reasonText = (reason: ActionReason) => {
        const values = object(reason.values);
        return t(`action_provider.reasons.${reason.code}`, {...values,
            ...(typeof values.field === "string" ? {field: t(`action_provider.fields.${values.field}`, {defaultValue: values.field})} : {}),
        });
    };
    const label = (action: ActionCandidate) => {
        const params = object(action.params);
        const effect = object(params.effect);
        const raw = Number(effect.id ?? 0);
        const amulet = amuletById.get(Math.floor(raw / 10));
        const badgeId = Number(effect.badgeId ?? object(effect.badge).id ?? 0);
        const badge = badgeById.get(badgeId);
        const name = raw ? (raw % 10 === 1 ? amulet?.plus_name : amulet?.name) || amulet?.name || t("action_provider.amulet_id", {id: raw}) : "";
        const title = action.kind === "bonfire" ? t(`action_provider.bonfire.${params.id}`, {defaultValue: t("action_provider.choice_id", {id: params.id})})
            : action.kind === "select_character" ? t("character_health.portrait_alt", {id: params.id})
            : action.kind === "shop_upgrade" ? t(`action_provider.upgrades.${params.upgrade_id}`, {defaultValue: t("action_provider.upgrade_id", {id: params.upgrade_id})})
            : action.kind === "advanced_upgrade" ? t("action_provider.upgrade_id", {id: params.upgrade_id})
            : action.kind === "buy_pack" ? t("action_provider.pack_id", {id: params.goods_id})
            : action.kind === "buy_rune" ? t("action_provider.rune_id", {id: params.rune_id}) : name;
        const details = [badgeId ? badge?.name || t("action_provider.badge_id", {id: badgeId}) : "",
            params.uid != null ? t("action_provider.uid", {id: params.uid}) : "",
            params.index != null ? t("action_provider.candidate_index", {index: Number(params.index) + 1}) : "",
            Array.isArray(params.uids) && params.uids.length ? t("action_provider.sacrifice", {ids: params.uids.join(", ")}) : "",
            action.kind === "trade" && Number(params.hp_cost) > 0 ? t("action_provider.trade_hp", {cost: params.hp_cost}) : "",
        ].filter(Boolean);
        return <div className={styles.actionName}>
            <span className="ms" aria-hidden="true">{icons[action.kind] ?? "touch_app"}</span>
            <div><strong>{t(`action_provider.actions.${action.kind}`)}{title && <span> · {title}</span>}</strong>
                {details.length > 0 && <small>{details.join(" · ")}</small>}</div>
        </div>;
    };
    const rowContent = (action: ActionCandidate) => {
        return <>
        <div className={styles.actionInfo}>
            {label(action)}
            {action.reasons.length > 0 && <p className={styles.reason}>{action.reasons.map(reasonText).join(t("action_provider.reason_separator"))}</p>}
        </div>
        <span className={styles.actionMeta}>
            {action.cost != null && <span className={styles.cost}><span className="ms" aria-hidden="true">{action.cost_resource === "hp" ? "favorite" : "stars"}</span><span>{action.cost}</span><small>{t(`action_provider.${action.cost_resource}`)}</small></span>}
            <span className={`${styles.availability} ${styles[action.availability]}`}><span className="ms" aria-hidden="true">{pending === action.id ? "hourglass_top" : {available: "play_arrow", unavailable: "block", unknown: "help"}[action.availability]}</span>{t(pending === action.id ? "action_provider.execution.pending" : `action_provider.${action.availability}`)}</span>
        </span>
        {action.availability === "available" && scoreBadge(action)}
    </>;
    };
    const rows = (items: ActionCandidate[]) => items.map(action => <li key={action.id}>
        {action.availability === "available" ? <button type="button" className={`${styles.row} ${styles.action}`}
            disabled={pending !== null || waiting || state?.session_id == null || state.revision == null}
            aria-busy={pending === action.id} onClick={() => void execute(action)}>
            {rowContent(action)}
        </button> : <div className={styles.row}>{rowContent(action)}</div>}
    </li>);
    const status = report?.status ?? "waiting_state";
    return <section className={`panel ${styles.panel}`} aria-label={t("action_provider.title")}>
        <div className={styles.header}>
            <h3 className="panel-title"><span className="ms" aria-hidden="true">rule</span>{t("action_provider.title")}</h3>
            <div className={styles.headerActions}>
                {report && report.stage > 0 && <span className={styles.context}>{t(`action_provider.stages.${report.stage}`, {defaultValue: t("action_provider.stage_id", {id: report.stage})})}
                    {report.interrupt === 1000 && report.stage !== 2 && report.stage !== 16 && <> · {t("action_provider.pack_interrupt")}</>}
                    {state?.level ? <> · {formatLevelIdToLabel(state.level, state.source, state.node)}</> : null}</span>}
                <button type="button" ref={detailsButton} className="nav-btn" aria-haspopup="dialog" aria-expanded={detailsOpen} onClick={() => setDetailsOpen(true)}>
                    <span className="ms" aria-hidden="true">receipt_long</span>{t("action_provider.scoring.details")}
                </button>
            </div>
        </div>
        {feedback && <p className={`${styles.message} ${feedback.error ? styles.unavailable : styles.available}`} role={feedback.error ? "alert" : "status"}>
            <span className="ms" aria-hidden="true">{feedback.error ? "error" : "check_circle"}</span>{feedback.text}
        </p>}
        {waiting && <p className={styles.message} role="status">{t("action_provider.execution.awaiting_state")}</p>}
        {status !== "ready" && <p className={styles.message} role="status"><span className="ms" aria-hidden="true">info</span>{t(`action_provider.status.${status}`)}</p>}
        {(report?.issues ?? []).map((reason, index) => <p className={styles.message} key={index}><span className="ms" aria-hidden="true">help</span>{reasonText(reason)}</p>)}
        {actions.length > 0 && <>
            <div className={styles.filters} role="group" aria-label={t("action_provider.filter")}>
                {(["all", "available", "unavailable", "unknown"] as const).map(value => <button key={value} className="nav-btn" aria-pressed={filter === value} onClick={() => setFilter(value)}>
                    <span className="ms" aria-hidden="true">{{all: "list", available: "check_circle", unavailable: "block", unknown: "help"}[value]}</span>{t(`action_provider.${value}`)}<span>{counts[value]}</span>
                </button>)}
            </div>
            {visible.length ? <ul className={styles.list}>{rows(visible)}</ul> : <p className={styles.message}>{t("action_provider.no_matches")}</p>}
        </>}
        {status === "ready" && actions.length === 0 && <p className={styles.message}>{t("action_provider.no_actions")}</p>}
        {globalActions.length > 0 && <div className={styles.global}>
            <h4>{t("action_provider.global")}</h4><ul className={styles.list}>{rows(globalActions)}</ul>
        </div>}
        <Modal open={detailsOpen} onClose={closeDetails} title={t("action_provider.scoring.details")} width={760}>
            {detailsOpen && <div className={styles.details}>
                {scoredActions.length === 0 && <p className={styles.message}>{t("action_provider.scoring.empty")}</p>}
                {scoredActions.map(action => {
                    const score = scores.get(action.id);
                    return <section key={action.id} className={styles.detailCard}>
                        <div className={styles.detailHeader}>{label(action)}{scoreBadge(action)}</div>
                        {score?.total === null && <p className={styles.message}>{t("action_provider.scoring.out_of_range")}</p>}
                        {score?.rules.length ? <ul className={styles.rules}>
                            {score.rules.map(rule => <li key={rule.rule_id}>
                                <div><strong>{t(`action_provider.scoring.rules.${rule.rule_id}`)}</strong>
                                    <p>{rule.score == null ? t("action_provider.scoring.ignored", {reason: t(`action_provider.scoring.reasons.${rule.reason.code}`, {...object(rule.reason.values)})})
                                        : t(`action_provider.scoring.reasons.${rule.reason.code}`, {...object(rule.reason.values)})}</p></div>
                                <strong className={`${styles.ruleScore} ${scoreTone(rule.score ?? 0)}`}>{signed(rule.score ?? 0)}</strong>
                            </li>)}
                        </ul> : <p className={styles.message}>{t("action_provider.scoring.no_rules")}</p>}
                    </section>;
                })}
            </div>}
        </Modal>
    </section>;
}
