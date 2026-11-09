import {useTranslation} from "react-i18next";
import type {GameStateData} from "../lib/gamestate";
import {formatLevelIdToLabel} from "../lib/levelFormat";
import type {ActionExecution} from "../lib/useActionExecution";
import AmuletBar from "./AmuletBar";
import CandidateBar from "./CandidateBar";
import GoodsBar from "./GoodsBar";
import styles from "./AutoRunnerInventory.module.css";

export default function AutoRunnerInventory({state, execution, forbidRemake = false}: {state: GameStateData | null; execution: ActionExecution; forbidRemake?: boolean}) {
    const {t} = useTranslation();
    const amulets = state?.effect_list ?? [];
    const candidates = state?.candidate_effect_list ?? [];
    const goods = state?.goods ?? [];
    const remake = state?.action_report?.global_actions.find(action => action.kind === "remake");

    return <div className={styles.inventory}>
        <section className={`panel ${styles.group} ${styles.owned}`} aria-labelledby="autorun-amulets">
            <h3 className={styles.heading} id="autorun-amulets">
                <span className="ms" aria-hidden="true">extension</span>{t("amulet")}
                <span className={styles.count}>{amulets.length}</span>
                {state?.level ? <span className={styles.level} title={t("score.current_level")}>
                    <span className="ms" aria-hidden="true">flag</span>{formatLevelIdToLabel(state.level, state.source, state.node)}
                </span> : null}
                <button type="button" className={styles.remake}
                    disabled={forbidRemake || execution.disabled || remake?.availability !== "available"}
                    aria-busy={!!remake && execution.pending === remake.id}
                    title={forbidRemake ? t("vine.remake_forbidden") : execution.waiting ? t("action_provider.execution.awaiting_state") : t("action_provider.actions.remake")}
                    onClick={() => {if (remake) void execution.execute(remake);}}>
                    <span className="ms" aria-hidden="true">{remake && execution.pending === remake.id ? "hourglass_top" : "restart_alt"}</span>
                    {t(remake && execution.pending === remake.id ? "action_provider.execution.pending" : "action_provider.actions.remake")}
                </button>
            </h3>
            <AmuletBar items={amulets} scale={0.38}/>
        </section>
        <section className={`panel ${styles.group} ${styles.candidates}`} aria-labelledby="autorun-candidates">
            <h3 className={styles.heading} id="autorun-candidates">
                <span className="ms" aria-hidden="true">style</span>{t("candidate_amulet")}
                <span className={styles.count}>{candidates.length}</span>
            </h3>
            <CandidateBar candidates={candidates} ownedAmulets={amulets} scale={0.38} max={candidates.length}/>
        </section>
        <section className={`panel ${styles.group}`} aria-labelledby="autorun-goods">
            <h3 className={styles.heading} id="autorun-goods">
                <span className="ms" aria-hidden="true">shopping_bag</span>{t("goods")}
                <span className={styles.count}>{goods.length}</span>
            </h3>
            <GoodsBar items={goods} scale={0.38} max={goods.length}/>
        </section>
    </div>;
}
