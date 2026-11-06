import React from "react";
import {useTranslation} from "react-i18next";
import type {MagicVineGame, MagicVineStats as Stats} from "../bindings";
import {getMagicVineStats, onBackendEvent, subscribeBackendEvent} from "../lib/ipc";
import {formatLevelIdToLabel} from "../lib/levelFormat";
import Modal from "./Modal";
import styles from "./MagicVineStats.module.css";
import {hasObtainedMagicVine, highestTileScore} from "../lib/magicVineMetrics";
import {formatNumberWithScientificThreshold} from "../lib/bigNumber";

const obtained = (game: MagicVineGame) => game.pack_obtained + game.shop_obtained + game.regeneration_obtained + game.other_obtained;

export default function MagicVineStats({tileScoreMap, amulets}: {tileScoreMap?: Record<string, string>; amulets?: readonly {id: number}[]}) {
    const {t, i18n} = useTranslation();
    const [stats, setStats] = React.useState<Stats>({current: null, history: [], loading: true});
    const [open, setOpen] = React.useState(false);
    const [error, setError] = React.useState(false);
    const close = React.useCallback(() => setOpen(false), []);
    const highest = React.useMemo(() => highestTileScore(tileScoreMap), [tileScoreMap]);
    React.useEffect(() => {
        let active = true;
        let version = 0;
        let unlisten = () => {};
        const refresh = async () => {
            const requested = version;
            try {
                const value = await getMagicVineStats();
                if (active && requested === version) { setStats(value); setError(false); }
            } catch { if (active && requested === version) setError(true); }
        };
        const stopResync = subscribeBackendEvent("resync_required", () => {void refresh();});
        void onBackendEvent("magic_vine_stats", (value) => {
            if (!active) return;
            version += 1;
            setError(false);
            setStats(value);
        }).then(listener => {
            if (!active) { listener(); return; }
            unlisten = listener;
            void refresh();
        }).catch(() => { if (active) setError(true); });
        return () => { active = false; unlisten(); stopResync(); };
    }, []);
    const visible = hasObtainedMagicVine(stats.current, amulets);
    React.useEffect(() => {if (!visible) setOpen(false);}, [visible]);
    if (!visible) return null;

    const current = stats.current;
    const hasOther = stats.history.some(game => game.other_obtained > 0);
    const columns = ["game", "pack_obtained", "shop_obtained", "regeneration_obtained", ...(hasOther ? ["other_obtained"] : []), "obtained", "score_triggers", "expansion_extra", "farming_average", "shops_purchases"];
    const cells = (game: MagicVineGame) => <>
        <td>{game.pack_obtained}</td><td>{game.shop_obtained}</td><td>{game.regeneration_obtained}</td>{hasOther && <td>{game.other_obtained}</td>}
        <td>{game.pack_obtained + game.shop_obtained + game.regeneration_obtained + game.other_obtained}</td>
        <td><strong>{game.score_triggers}</strong></td><td>+{game.expansion_extra_triggers}</td>
        <td>{game.farming_rounds > 0 ? (game.farming_obtained / game.farming_rounds).toFixed(2) : "—"}<small>{t("magic_vine.farming_counts", {count: game.farming_rounds ?? 0, obtained: game.farming_obtained ?? 0})}</small></td>
        <td>{game.shops} / {game.shop_obtained}</td>
    </>;
    return <>
        <div className="home-record-row is-up" data-testid="magic-vine-highest-score">
            <div className="home-record-content">
                <div className="home-record-label">{t("magic_vine.highest_score")}</div>
                <div className="home-record-value-line"><span className="home-record-value">{highest == null ? "—" : formatNumberWithScientificThreshold(highest, 2)}</span></div>
            </div>
        </div>
        <button className={styles.summary} onClick={() => setOpen(true)} aria-label={t("magic_vine.open")}>
            <span className={styles.title}><span className="ms" aria-hidden="true">psychiatry</span>{t("magic_vine.title")}<span className="ms" aria-hidden="true">chevron_right</span></span>
            {error ? <span>{t("magic_vine.load_failed")}</span> : <span className={styles.metrics}>
                <span><small>{t("magic_vine.obtained")}</small><strong>{current ? obtained(current) : "—"}</strong></span>
                <span><small>{t("magic_vine.per_round")}</small><strong>{current && current.farming_rounds > 0 ? (current.farming_obtained / current.farming_rounds).toFixed(2) : "—"}</strong></span>
                <span><small>{t("magic_vine.shops_purchases")}</small><strong>{current ? `${current.shops} / ${current.shop_obtained}` : "—"}</strong></span>
            </span>}
        </button>
        <Modal open={open} onClose={close} title={t("magic_vine.history_title")} width={1160}
            actions={current && <span className={styles.phase}>{t("magic_vine.farming_state", {state: t(`magic_vine.states.${current.farming_status || "unformed"}`)})}</span>}>
            {stats.loading && <p className={styles.note}>{t("magic_vine.loading")}</p>}
            <div className={styles.tableWrap}>
                <table className={styles.table}>
                    <thead><tr>{columns.map(key => <th key={key}>{t(`magic_vine.${key}`)}</th>)}</tr></thead>
                    <tbody>{stats.history.map(game => <tr key={game.started_at}>
                        <td><span>{new Date(game.started_at).toLocaleString(i18n.language)}</span><small title={game.incomplete ? t("magic_vine.incomplete") : undefined}>{formatLevelIdToLabel(game.level, "packet", game.node)} · {t(game.finished ? "magic_vine.finished" : "magic_vine.playing")}{game.incomplete && ` · ${t("magic_vine.partial_label")}`}</small>
                            {(game.farming_periods ?? []).map((period, index) => <small key={index}>{formatLevelIdToLabel(period.start_level, "packet", period.start_node)} → {formatLevelIdToLabel(period.end_level, "packet", period.end_node)} · {t("magic_vine.farming_counts", {count: period.rounds, obtained: period.obtained})}</small>)}
                        </td>
                        {cells(game)}
                    </tr>)}</tbody>
                </table>
            </div>
            {stats.history.length === 0 && <p className={styles.note}>{t("magic_vine.empty")}</p>}
        </Modal>
    </>;
}
