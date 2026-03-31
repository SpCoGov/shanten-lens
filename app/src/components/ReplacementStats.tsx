import React, {useMemo} from "react";
import "../styles/theme.css";
import Tile from "./Tile";
import styles from "./WallStats.module.css";
import {t} from "i18next";
import {keyToReadable} from "./WallStats";

export interface ReplacementStatsProps {
    replacementTiles: string[];
    usedCount: number;
    className?: string;
    headerSlot?: React.ReactNode;
}

function normalize(raw: string): string {
    const s = raw.trim();
    if (/^[0-9][mpsz]$/.test(s)) return s;
    return s;
}

function emitHover(tile: string | null) {
    window.dispatchEvent(new CustomEvent("shanten:hover-tile", {detail: tile}));
}

function valueSortNumber(v: number): number {
    return v === 0 ? 5.1 : v;
}

function sortKey(a: string, b: string): number {
    const suitOrder: Record<string, number> = {m: 0, p: 1, s: 2, z: 3};
    const sa = a[a.length - 1];
    const sb = b[b.length - 1];
    if (sa !== sb) return (suitOrder[sa] ?? 9) - (suitOrder[sb] ?? 9);
    const va = Number(a.slice(0, -1));
    const vb = Number(b.slice(0, -1));
    return valueSortNumber(va) - valueSortNumber(vb);
}

export default function ReplacementStats({replacementTiles, usedCount, className, headerSlot}: ReplacementStatsProps) {
    const list = useMemo(() => {
        const remain = replacementTiles.slice(Math.max(0, usedCount));
        const map = new Map<string, {count: number; sample: string}>();
        for (const tile of remain) {
            const key = normalize(tile);
            const cur = map.get(key);
            if (cur) cur.count += 1;
            else map.set(key, {count: 1, sample: tile});
        }
        return Array.from(map.entries())
            .sort((a, b) => sortKey(a[0], b[0]))
            .map(([key, value]) => ({
                key,
                sample: value.sample,
                readable: keyToReadable(key),
                count: value.count,
            }));
    }, [replacementTiles, usedCount]);

    return (
        <aside className={[styles.wrap, className].filter(Boolean).join(" ")}>
            <div className={`mj-panel ${styles.panel}`}>
                <div className={styles.header}>
                    {headerSlot ?? <div className={styles.title}>{t("replacement_stats.title")}</div>}
                </div>

                <div className={styles.list}>
                    {list.length === 0 ? (
                        <div className={styles.empty}>{t("replacement_stats.empty")}</div>
                    ) : (
                        list.map(({key, sample, readable, count}) => (
                            <div
                                className={styles.item}
                                key={key}
                                onMouseEnter={() => emitHover(sample)}
                                onMouseLeave={() => emitHover(null)}
                                onClick={() => emitHover(sample)}
                            >
                                <div className={styles.tileBox}>
                                    <Tile
                                        tile={sample}
                                        dim={false}
                                        hoveredTile={null}
                                        setHoveredTile={(tile) => emitHover(tile || null)}
                                        width={44}
                                        height={60}
                                    />
                                </div>
                                <div className={styles.meta}>
                                    <div className={styles.name}>{readable}</div>
                                    <div className={styles.subtext}>{t("replacement_stats.still_available")}</div>
                                </div>
                                <div className={styles.count}>×{count}</div>
                            </div>
                        ))
                    )}
                </div>
            </div>
        </aside>
    );
}
