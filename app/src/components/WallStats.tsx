import React, { useMemo } from "react";
import "../styles/theme.css";
import Tile from "./Tile";
import styles from "./WallStats.module.css";

export interface WallStatsProps {
    wallTiles: string[];
    className?: string;
}

/** 归一化：'1m/m1' -> 'm1'；'东' -> 'z1'；'0p' -> 'p0'；'4z' -> 'z4' */
function normalize(raw: string): string {
    const s = raw.trim();
    const honorCN: Record<string, string> = { 东: "z1", 南: "z2", 西: "z3", 北: "z4", 白: "z5", 发: "z6", 中: "z7" };
    if (honorCN[s]) return honorCN[s];
    if (/^[mps][0-9]$/.test(s)) return s;                // m0..m9 / p0..p9 / s0..s9
    if (/^[0-9][mps]$/.test(s)) return `${s[1]}${s[0]}`;  // 1m..9m / 0p..9p
    if (/^z[1-7]$/.test(s)) return s;                     // z1..z7
    if (/^[1-7]z$/.test(s)) return `z${s[0]}`;            // 1z..7z -> z1..z7
    return s; // 其他编码原样
}

/** 显示名：0→赤五X；1..9→汉字X；字牌→东南西北白发中（兜底兼容 'Nz'） */
function keyToReadable(key: string): string {
    const suit = key[0];
    const valStr = key.slice(1);
    const v = Number(valStr);
    const suitMap: Record<string, string> = { m: "万", p: "筒", s: "索" };
    const cn = ["零","一","二","三","四","五","六","七","八","九"];

    if (suit === "z") {
        const honors = ["东","南","西","北","白","发","中"];
        return honors[(v - 1 + 7) % 7] ?? key;
    }
    if (/^[1-7]z$/.test(key)) {
        const honors = ["东","南","西","北","白","发","中"];
        const n = Number(key[0]);
        return honors[n - 1] ?? key;
    }

    if (suitMap[suit]) {
        if (v === 0) return `赤五${suitMap[suit]}`;
        if (v >= 1 && v <= 9) return `${cn[v]}${suitMap[suit]}`;
    }
    return key;
}

/** 同花色排序值：0（赤五）紧随 5 之后，以 5.1 处理 */
function valueSortNumber(v: number): number {
    return v === 0 ? 5.1 : v;
}

/** 排序：m→p→s→z；同花色数值升序，赤五紧跟五 */
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

/** 等价组：普通五 ↔ 赤五；其他牌仅自身 */
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

/** 发出两个事件：单值兼容 + 等价组（数组） */
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
                    <div className={styles.title}>牌山统计</div>
                </div>

                <div className={styles.list}>
                    {list.length === 0 ? (
                        <div className={styles.empty}>当前无可摸牌</div>
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
                                        hoveredTile={null}               // 侧栏不吃外部高亮
                                        setHoveredTile={(t) => emitHover(t || null)}
                                        width={44}
                                        height={60}
                                    />
                                </div>
                                <div className={styles.meta}>
                                    <div className={styles.name}>{readable}</div>
                                    <div className={styles.subtext}>还能摸到</div>
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