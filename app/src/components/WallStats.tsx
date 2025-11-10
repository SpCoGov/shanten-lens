import React, { useMemo } from "react";
import "../styles/theme.css";
import Tile from "./Tile";
import styles from "./WallStats.module.css";
import i18next, {t} from "i18next";

export interface WallStatsProps {
    wallTiles: string[];
    className?: string;
}

function normalize(raw: string): string {
    const s = raw.trim();
    const honorCN: Record<string, string> = { 东: "z1", 南: "z2", 西: "z3", 北: "z4", 白: "z5", 发: "z6", 中: "z7" };
    const honorJP: Record<string, string> = { 東: "z1", 南: "z2", 西: "z3", 北: "z4", 白: "z5", 発: "z6", 中: "z7", 發: "z6" };
    if (honorCN[s]) return honorCN[s];
    if (honorJP[s]) return honorJP[s];
    if (/^[mps][0-9]$/.test(s)) return s;
    if (/^[0-9][mps]$/.test(s)) return `${s[1]}${s[0]}`;
    if (/^z[1-7]$/.test(s)) return s;
    if (/^[1-7]z$/.test(s)) return `z${s[0]}`;
    return s;
}

export function keyToReadable(key: string, i18n = i18next): string {
    const t = i18n.t.bind(i18n);
    const k = normalize(key);
    const suitKey = k[0];
    const valStr = k.slice(1);
    const v = Number(valStr);

    const suitName = t(`tile.suits.${suitKey}`, { defaultValue: "" }) as string;
    const honors = t("tile.honors", { returnObjects: true }) as string[];
    const numbers = t("tile.numbers", { returnObjects: true }) as string[];

    if (suitKey === "z") {
        const idx = Math.min(Math.max(v - 1, 0), 6);
        return honors[idx] ?? k;
    }
    if (/^[1-7]z$/.test(key)) {
        const idx = Number(key[0]) - 1;
        return honors[idx] ?? key;
    }

    if (suitName) {
        if (v === 0) {
            return t("tile.format.red_five", { suit: suitName }) as string;
        }
        if (v >= 1 && v <= 9) {
            const numWord = numbers[v] ?? String(v);
            return t("tile.format.number_suit", { num: numWord, suit: suitName }) as string;
        }
    }

    return key;
}

function valueSortNumber(v: number): number {
    return v === 0 ? 5.1 : v;
}

function sortKey(a: string, b: string): number {
    const order: Record<string, number> = { m: 0, p: 1, s: 2, z: 3 };
    const na = normalize(a);
    const nb = normalize(b);
    const sa = na[0], sb = nb[0];
    if (sa !== sb) return (order[sa] ?? 9) - (order[sb] ?? 9);
    const va = Number(na.slice(1));
    const vb = Number(nb.slice(1));
    const knownA = /^[mps][0-9]$/.test(na) || /^z[1-7]$/.test(na);
    const knownB = /^[mps][0-9]$/.test(nb) || /^z[1-7]$/.test(nb);
    if (knownA && knownB) return valueSortNumber(va) - valueSortNumber(vb);
    if (knownA) return -1;
    if (knownB) return 1;
    return na.localeCompare(nb);
}

function eqGroup(tile: string): string[] {
    const t = normalize(tile);
    if (/^[mps][0-9]$/.test(t)) {
        const suit = t[0];
        const v = Number(t.slice(1));
        if (v === 0) return [t, `${suit}5`];
        if (v === 5) return [t, `${suit}0`];
    }
    return [t];
}

function emitHover(tile: string | null) {
    window.dispatchEvent(new CustomEvent("shanten:hover-tile", { detail: tile }));
    const group = tile ? eqGroup(tile) : [];
    window.dispatchEvent(new CustomEvent("shanten:hover-tile-eq", { detail: group }));
}

export default function WallStats({ wallTiles, className }: WallStatsProps) {
    const list = useMemo(() => {
        const map = new Map<string, { count: number; sample: string }>();
        for (const t of wallTiles) {
            const k = normalize(t);
            const cur = map.get(k);
            if (cur) cur.count += 1;
            else map.set(k, { count: 1, sample: t });
        }
        return Array.from(map.entries())
            .sort((a, b) => sortKey(a[0], b[0]))
            .map(([key, v]) => ({
                key,
                sample: v.sample,
                readable: keyToReadable(key),
                count: v.count,
            }));
    }, [wallTiles]);

    return (
        <aside className={[styles.wrap, className].filter(Boolean).join(" ")}>
            <div className={`mj-panel ${styles.panel}`}>
                <div className={styles.header}>
                    <div className={styles.title}>{t("wall_stats.title")}</div>
                </div>

                <div className={styles.list}>
                    {list.length === 0 ? (
                        <div className={styles.empty}>{t("wall_stats.empty")}</div>
                    ) : (
                        list.map(({ key, sample, readable, count }) => (
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
                                        setHoveredTile={(t) => emitHover(t || null)}
                                        width={44}
                                        height={60}
                                    />
                                </div>
                                <div className={styles.meta}>
                                    <div className={styles.name}>{readable}</div>
                                    <div className={styles.subtext}>{t("wall_stats.still_drawable")}</div>
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