import React from "react";
import styles from "./AdvisorPanel.module.css";
import Tile from "./Tile";

export type PlanData = {
    status: "win_now" | "plan" | "impossible";
    draws_needed?: number;
    target14?: string[];
    discards?: number[];
    mode?: string;
    reason?: string;
};

export default function AdvisorPanel({
                                         suuAnkou,
                                         chiitoi,
                                         resolveFace,
                                     }: {
    suuAnkou: PlanData | null;
    chiitoi: PlanData | null;
    resolveFace?: (id: number) => string | null;
}) {
    return (
        <aside className={styles.wrap}>
            <StrategyCard title="四暗刻" data={suuAnkou} resolveFace={resolveFace} />
            <StrategyCard title="七対子" data={chiitoi} resolveFace={resolveFace} />
        </aside>
    );
}

function StrategyCard({
                          title,
                          data,
                          resolveFace,
                          badge
                      }: {
    title: string;
    data: PlanData | null;
    resolveFace?: (id: number) => string | null;
    badge?: string;
    onlyShowImpossibleWhenMelded?: boolean;
}) {
    if (!data) {
        return (
            <section className={styles.card}>
                <div className={styles.cardHead}>
                    <h3>{title}</h3>
                    {badge && badge.trim() && <span className={styles.badgeMuted}>{badge}</span>}
                </div>
                <div className={styles.cardBodyMuted}>等待后端结果…</div>
            </section>
        );
    }

    const isWinNow = data.status === "win_now";
    const isPlan = data.status === "plan";
    const isImpossible = data.status === "impossible";

    const reasonLower = (data.reason || "").toLowerCase();
    const isMenzenOnlyWord = isImpossible && reasonLower === "hand-must-be-14";

    const firstDiscardId =
        isPlan && data.discards && data.discards.length > 0 ? data.discards[0] : null;
    const firstDiscardFace = firstDiscardId != null && resolveFace ? resolveFace(firstDiscardId) : null;

    return (
        <section className={styles.card}>
            <div className={styles.cardHead}>
                <h3>{title}</h3>
                {badge && badge.trim() && <span className={styles.badgeBlue}>{badge}</span>}
            </div>

            {isWinNow ? (
                <div className={styles.okBand}>
                    <div className={styles.bandOkText}>当前可立刻和</div>
                </div>
            ) : isImpossible ? (
                <div className={styles.bandSingle}>
                    <div className={styles.bandLabel}>还需摸</div>
                    <div className={styles.bandValue}>{isMenzenOnlyWord ? "門前のみ" : "无解"}</div>
                </div>
            ) : (
                // 可规划：左右两列 + 分隔符 + 推荐打牌
                <div className={styles.band}>
                    <div className={styles.bandLeft}>
                        <div className={styles.bandLabel}>还需摸</div>
                        <div className={styles.bandValue}>
                            {formatDrawsNeeded(
                                typeof data.draws_needed === "number" ? data.draws_needed : null
                            )}
                        </div>
                    </div>

                    <div className={styles.vbar} />

                    <div className={styles.bandRight}>
                        <div className={styles.bandActionLabel}>推荐打牌</div>
                        {firstDiscardId != null ? (
                            <div className={styles.actionChip}>
                <span className={`${styles.tilePill} ${styles.tileReset} ${styles.tileRound}`}>
                  <Tile tile={firstDiscardFace || "-"} />
                </span>
                                <span className={styles.chipText}>id: {firstDiscardId}</span>
                            </div>
                        ) : (
                            <div className={`${styles.actionChip} ${styles.chipDisabled}`}>—</div>
                        )}
                    </div>
                </div>
            )}
        </section>
    );
}

function Metric({ label, value }: { label: string; value: string }) {
    return (
        <div className={styles.metric}>
            <div className={styles.metricLabel}>{label}</div>
            <div className={styles.metricValue}>{value}</div>
        </div>
    );
}

function TilePill({ face }: { face: string | null }) {
    if (!face) return <span>-</span>;
    return (
        <span className={`${styles.tilePill} ${styles.tileReset} ${styles.tileRound}`}>
      <Tile tile={face} />
    </span>
    );
}

function formatDrawsNeeded(n: number | null) {
    if (n === null || n === undefined) return "无解";
    return String(n);
}
