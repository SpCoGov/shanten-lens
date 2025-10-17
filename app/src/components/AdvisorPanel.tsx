import React from "react";
import styles from "./AdvisorPanel.module.css";
import Tile from "./Tile";

export type ChiitoiData = {
    policy: "speed" | "count";
    applicable: boolean;
    could_win_now: boolean;
    sacrifice_for_count: boolean;
    win_now: boolean;
    discard_id: number | null;
    discard_tile: string | null;
    discard_tile_raw: string | null;
    draws_needed: number | null;
    uke_ire_total: number | null;
    uke_detail: Record<string, number> | null;
};

export default function AdvisorPanel({
                                         speed,
                                         count,
                                     }: {
    speed: ChiitoiData | null;
    count: ChiitoiData | null;
}) {
    return (
        <aside className={styles.wrap}>
            <StrategyCard data={speed} variant="speed"/>
            <StrategyCard data={count} variant="count"/>
            <Legend/>
        </aside>
    );
}

function StrategyCard({
                          data,
                          variant,
                      }: {
    data: ChiitoiData | null;
    variant: "speed" | "count";
}) {
    const header = variant === "speed" ? "速度优先" : "次数优先";
    const badge = variant === "speed" ? "最快" : "受入最多";

    if (!data) {
        return (
            <section className={styles.card}>
                <div className={styles.cardHead}>
                    <h3>{header}</h3>
                    <span className={styles.badgeMuted}>{badge}</span>
                </div>
                <div className={styles.cardBodyMuted}>等待后端结果…</div>
            </section>
        );
    }

    const canWinNow = data.applicable && data.win_now;
    const showDiscard = data.applicable && !data.win_now && data.discard_id != null;
    const showUke = variant === "count";

    return (
        <section className={styles.card}>
            <div className={styles.cardHead}>
                <h3>{header}</h3>
                <span className={variant === "speed" ? styles.badgeBlue : styles.badgeGreen}>{badge}</span>
            </div>

            {!data.applicable ? (
                <div className={styles.cardBodyMuted}>当前不适用七对（需要门清）</div>
            ) : canWinNow ? (
                <div className={styles.winNow}>
                    <div className={styles.winNowTitle}>当前可立刻和（七对）</div>
                </div>
            ) : (
                <div className={styles.cardBody}>
                    {showDiscard ? (
                        <div className={styles.rowBetween}>
                            <div>
                                <div className={styles.label}>
                                    {variant === "count" && data.could_win_now
                                        ? "可选：扩大受入（放弃即胡）"
                                        : "推荐打牌"}
                                </div>
                                <div className={styles.tileRow}>
                                    <TilePill face={data.discard_tile} raw={data.discard_tile_raw}/>
                                    <span className={styles.idHint}>id: {data.discard_id}</span>
                                </div>
                            </div>
                        </div>
                    ) : null}

                    <div className={styles.grid2}>
                        <Metric label="还需摸" value={formatDrawsNeeded(data.draws_needed)}/>
                        <Metric
                            label="受入张数"
                            value={showUke && data.uke_ire_total != null ? String(data.uke_ire_total) : "-"}
                        />
                    </div>

                    {showUke && data.uke_detail && Object.keys(data.uke_detail).length > 0 ? (
                        <div className={styles.ukeBox}>
                            <div className={styles.label}>受入明细</div>
                            <UkeDetail detail={data.uke_detail}/>
                        </div>
                    ) : null}
                </div>
            )}
        </section>
    );
}

function Metric({label, value}: { label: string; value: string }) {
    return (
        <div className={styles.metric}>
            <div className={styles.metricLabel}>{label}</div>
            <div className={styles.metricValue}>{value}</div>
        </div>
    );
}

function UkeDetail({detail}: { detail: Record<string, number> }) {
    const entries = Object.entries(detail).sort((a, b) => b[1] - a[1]);
    const total = entries.reduce((s, [, n]) => s + n, 0);
    return (
        <div className={styles.tableWrap}>
            <div className={styles.tableHead}>
                <div>牌面</div>
                <div className={styles.center}>张数</div>
                <div className={styles.right}>占比</div>
            </div>
            <div className={styles.tableBody}>
                {entries.map(([face, cnt], i) => (
                    <div key={face + i} className={styles.tableRow}>
                        <div className={styles.tileRow}>
                            <TileSmall face={face}/>
                            {face}
                        </div>
                        <div className={styles.center}>{cnt}</div>
                        <div className={styles.right}>{total ? Math.round((cnt / total) * 100) + "%" : "-"}</div>
                    </div>
                ))}
            </div>
        </div>
    );
}

function Legend() {
    return (
        <div className={styles.legend}>
            <span className={styles.info}>i</span>
            <div>
                <div>“受入张数”仅在次数优先显示。</div>
            </div>
        </div>
    );
}

function TilePill({face}: { face: string | null; raw: string | null }) {
    if (!face) return <span>-</span>;
    return (
        <span className={`${styles.tilePill} ${styles.tileReset} ${styles.tileRound}`}>
      <Tile tile={face}/>
    </span>
    );
}

function TileSmall({face}: { face: string }) {
    return (
        <span className={`${styles.tileSmall} ${styles.tileReset} ${styles.tileRoundSm}`}>
      <Tile tile={face}/>
    </span>
    );
}

function formatDrawsNeeded(n: number | null) {
    if (n === null || n === undefined) return "无解";
    if (n === 0) return "0";
    return String(n);
}