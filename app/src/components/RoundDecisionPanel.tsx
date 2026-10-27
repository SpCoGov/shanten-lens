import {useTranslation} from "react-i18next";
import type {PlayProfile} from "../lib/autoPlayConfig";
import type {RoundDecisionStatus} from "../lib/autoRunnerStore";
import Tile from "./Tile";
import styles from "./RoundDecisionPanel.module.css";

const statusIcons = {waiting: "hourglass_empty", searching: "manage_search", executing: "swap_horiz", winning: "celebration", blocked: "pause_circle", stopped: "pause"};

export default function RoundDecisionPanel({decision, running, profile}: {
    decision?: RoundDecisionStatus | null;
    running: boolean;
    profile?: PlayProfile | null;
}) {
    const {t, i18n} = useTranslation();
    const status = decision?.status === "blocked" ? "blocked" : running ? decision?.status ?? "waiting" : "stopped";
    const opening = decision?.strategy === "opening" || (!decision?.strategy && profile?.strategy === "opening");
    const reason = decision?.reason;
    const reasonKeys = reason ? [reason, `round.${reason}`, `custom_target.reason_${reason}`, `advisor.reason_${reason}`] : [];
    const reasonKey = reasonKeys.find(key => i18n.exists(key));
    const reasonText = reasonKey ? t(reasonKey) : reason;
    const metrics = [
        ...(opening ? [["pinzu_count", decision?.pinzu_count], ["score", decision?.score]] as const : []),
        ["win_count", decision?.win_count],
        ["draws_needed", decision?.draws_needed],
        ["actual_wins", decision?.actual_wins],
    ] as const;
    return <section className={`panel ${styles.panel}`} aria-label={t("round.title")}>
        <div className={styles.header}>
            <h3 className="panel-title"><span className="ms" aria-hidden="true">psychology</span>{t("round.title")}</h3>
            <span className={`${styles.status} ${styles[status] ?? ""}`} role="status"><span className="ms" aria-hidden="true">{statusIcons[status]}</span>{t(`round.status.${status}`)}</span>
        </div>
        <div className={styles.summary}>
            <div className={styles.objective}>
                <strong>{profile ? t(opening ? "round.opening_objective" : "round.full_objective") : t("autoplay.select_profile")}</strong>
                {(decision?.stage_name || decision?.hand_name) && <span>{[decision.stage_name, decision.hand_name].filter(Boolean).join(" · ")}</span>}
            </div>
            {metrics.some(([, value]) => value != null) && <dl className={styles.metrics}>
                {metrics.map(([name, value]) => value == null ? null : <div key={name}><dt>{t(`round.${name}`)}</dt><dd>{value.toLocaleString()}</dd></div>)}
            </dl>}
        </div>
        {!!decision?.target_faces?.length && <div className={styles.target}>
            <span>{t("round.target_hand")}</span>
            <div className={styles.tiles} role="img" aria-label={t("round.target_description", {tiles: decision.target_faces.join(" ")})}>
                {decision.target_faces.map((tile, index) => <Tile key={index} tile={tile} width={30} height={40} shadow={false}/>)}
            </div>
        </div>}
        {reasonText && <p className={styles.reason}>{reasonText}</p>}
        <p className={styles.hint}>{t("round.scope_hint")}</p>
    </section>;
}
