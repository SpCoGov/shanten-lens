import React from "react";
import {useTranslation} from "react-i18next";
import type {EffectItem} from "../lib/gamestate";
import {compareNumericStrings, formatLargeScaledNumber} from "../lib/bigNumber";
import {parseFixed2} from "../lib/scoreEngine";
import {useRegistry} from "../lib/registryStore";

const LEVEL_RECORDS_STORAGE_KEY = "sl-level-metric-records-v1";
const AVG_SOUZU_SCORE_METRIC_KEY = "avg_souzu_score";
const MAX_SOUZU_SCORE_METRIC_KEY = "max_souzu_score";
const AMULET_DATA_METRICS = [
    {regId: 229, key: "amulet_229_data0", labelKey: "level_records.amulet_229_data0"},
    {regId: 227, key: "amulet_227_data0", labelKey: "level_records.amulet_227_data0"},
    {regId: 157, key: "amulet_157_data0", labelKey: "level_records.amulet_157_data0"},
] as const;

type LevelMetricStore = Record<string, Record<string, string>>;

type CurrentLevelMetric = {
    key: string;
    label: string;
    current: string | null;
    amuletRegId?: number;
};

export type LevelRecordItem = {
    key: string;
    label: string;
    current: string;
    previousBest: string | null;
    best: string;
    delta: string | null;
    state: "meetsBest" | "below";
    amuletRegId?: number;
};

function readLevelMetricStore(): LevelMetricStore {
    try {
        const raw = localStorage.getItem(LEVEL_RECORDS_STORAGE_KEY);
        if (!raw) return {};
        const parsed = JSON.parse(raw);
        if (!parsed || typeof parsed !== "object") return {};
        return parsed as LevelMetricStore;
    } catch {
        return {};
    }
}

function writeLevelMetricStore(store: LevelMetricStore) {
    try {
        localStorage.setItem(LEVEL_RECORDS_STORAGE_KEY, JSON.stringify(store));
    } catch {
    }
}

function formatLevelLabel(level: number): string {
    if (level >= 1001) return `Ex${level - 1000}`;
    const chapter = Math.trunc(level / 100);
    const stage = level % 10;
    if (chapter > 0 && stage > 0) return `${chapter}-${stage}`;
    return String(level || "-");
}

function lookupTileScore(tileScoreMap: Record<string, string>, tile: string): string | null {
    if (Object.prototype.hasOwnProperty.call(tileScoreMap, tile)) return tileScoreMap[tile];
    const reversed = /^[0-9][mps]$/.test(tile) ? `${tile[1]}${tile[0]}` : tile;
    if (Object.prototype.hasOwnProperty.call(tileScoreMap, reversed)) return tileScoreMap[reversed];
    return null;
}

function computeAverageSouzuScore(tileScoreMap: Record<string, string>): string | null {
    let sum = 0n;
    for (let i = 1; i <= 9; i += 1) {
        const raw = lookupTileScore(tileScoreMap, `${i}s`);
        if (raw == null) return null;
        try {
            sum += parseFixed2(raw);
        } catch {
            return null;
        }
    }
    const divisor = 9n;
    const quotient = sum / divisor;
    const remainder = sum % divisor;
    return (quotient + (remainder * 2n >= divisor ? 1n : 0n)).toString();
}

function computeMaxSouzuScore(tileScoreMap: Record<string, string>): string | null {
    let max: bigint | null = null;
    for (let i = 1; i <= 9; i += 1) {
        const raw = lookupTileScore(tileScoreMap, `${i}s`);
        if (raw == null) continue;
        try {
            const parsed = parseFixed2(raw);
            if (max == null || parsed > max) max = parsed;
        } catch {
        }
    }
    return max == null ? null : max.toString();
}

function readEffectData0(item: EffectItem): string | null {
    const raw = Array.isArray(item.store) ? item.store[0] : null;
    if (raw == null) return null;
    try {
        return BigInt(String(raw)).toString();
    } catch {
        return null;
    }
}

function getAmuletData0Metric(amulets: EffectItem[], regId: number): string | null {
    let best: bigint | null = null;
    for (const item of amulets) {
        if (Math.floor(Number(item.id || 0) / 10) !== regId) continue;
        const raw = readEffectData0(item);
        if (raw == null) continue;
        const parsed = BigInt(raw);
        if (parsed === 100n || parsed === 1n) continue;
        if (best == null || parsed > best) best = parsed;
    }
    return best == null ? null : best.toString();
}

function formatRecordValue(value: string | null): string {
    if (value == null) return "-";
    try {
        return formatLargeScaledNumber(BigInt(value), 2, {humanDecimals: 2, scientificDecimals: 4});
    } catch {
        return "-";
    }
}

function formatRecordDelta(value: string | null): string | null {
    if (value == null) return null;
    try {
        const parsed = BigInt(value);
        if (parsed === 0n) return null;
        const sign = parsed > 0n ? "+" : "-";
        const abs = parsed > 0n ? parsed : -parsed;
        return `${sign}${formatLargeScaledNumber(abs, 2, {humanDecimals: 2, scientificDecimals: 4})}`;
    } catch {
        return null;
    }
}

function pad4(n: number) {
    return n.toString().padStart(4, "0");
}

export function useLevelRecordItems({
    level,
    tileScoreMap,
    amulets,
}: {
    level: number;
    tileScoreMap: Record<string, string>;
    amulets: EffectItem[];
}) {
    const {t} = useTranslation();

    const currentLevelMetrics = React.useMemo<CurrentLevelMetric[]>(() => [
        {
            key: AVG_SOUZU_SCORE_METRIC_KEY,
            label: t("level_records.avg_souzu_score"),
            current: computeAverageSouzuScore(tileScoreMap),
        },
        {
            key: MAX_SOUZU_SCORE_METRIC_KEY,
            label: t("level_records.max_souzu_score"),
            current: computeMaxSouzuScore(tileScoreMap),
        },
        ...AMULET_DATA_METRICS.map((metric) => ({
            key: metric.key,
            label: t(metric.labelKey),
            current: getAmuletData0Metric(amulets, metric.regId),
            amuletRegId: metric.regId,
        })),
    ], [amulets, tileScoreMap, t]);

    const [items, setItems] = React.useState<LevelRecordItem[]>([]);

    React.useEffect(() => {
        const store = readLevelMetricStore();
        const levelKey = String(level || "");
        const levelRecords = level ? (store[levelKey] ?? {}) : {};
        const nextItems: LevelRecordItem[] = [];
        let storeChanged = false;

        for (const metric of currentLevelMetrics) {
            if (metric.current == null) continue;

            if (!level) {
                nextItems.push({
                    key: metric.key,
                    label: metric.label,
                    current: metric.current,
                    previousBest: null,
                    best: metric.current,
                    delta: null,
                    state: "meetsBest",
                    amuletRegId: metric.amuletRegId,
                });
                continue;
            }

            const previousBest = levelRecords[metric.key] ?? null;
            let best = previousBest ?? metric.current;
            let delta: string | null = null;
            let state: LevelRecordItem["state"] = "meetsBest";

            if (previousBest == null) {
                levelRecords[metric.key] = metric.current;
                storeChanged = true;
            } else if (compareNumericStrings(metric.current, previousBest) >= 0) {
                delta = (BigInt(metric.current) - BigInt(previousBest)).toString();
                best = metric.current;
                levelRecords[metric.key] = metric.current;
                storeChanged = true;
            } else {
                delta = (BigInt(metric.current) - BigInt(previousBest)).toString();
                state = "below";
            }

            nextItems.push({
                key: metric.key,
                label: metric.label,
                current: metric.current,
                previousBest,
                best,
                delta,
                state,
                amuletRegId: metric.amuletRegId,
            });
        }

        if (level && storeChanged) {
            store[levelKey] = levelRecords;
            writeLevelMetricStore(store);
        }

        setItems(nextItems);
    }, [currentLevelMetrics, level]);

    return items;
}

export default function LevelRecordPanel({
    level,
    items,
}: {
    level: number;
    items: LevelRecordItem[];
}) {
    const {t} = useTranslation();
    const registry = useRegistry();
    if (items.length === 0) return null;

    return (
        <aside className="home-record-panel">
            <div className="home-record-kicker">{t("level_records.level_label", {level: formatLevelLabel(level)})}</div>
            <div className="home-record-list">
                {items.map((item) => {
                    const amulet = item.amuletRegId != null ? registry.amuletById.get(item.amuletRegId) : null;
                    const iconPath = amulet ? `/assets/amulet/fu_${pad4(amulet.icon_id)}.png` : null;
                    const showBest = item.state === "below";
                    const stateClass = item.state === "meetsBest" ? "is-up" : "is-down";
                    return (
                        <div key={item.key} className={`home-record-row ${stateClass}`}>
                            {iconPath ? (
                                <img
                                    className="home-record-icon"
                                    src={iconPath}
                                    alt={amulet?.name ?? item.label}
                                    draggable={false}
                                />
                            ) : null}
                            <div className="home-record-content">
                                <div className="home-record-label">{item.label}</div>
                                <div className="home-record-value-line">
                                    <span className="home-record-value">{formatRecordValue(item.current)}</span>
                                    {formatRecordDelta(item.delta) ? (
                                        <span className="home-record-delta">{formatRecordDelta(item.delta)}</span>
                                    ) : null}
                                </div>
                                {showBest ? (
                                    <div className="home-record-meta">
                                        <span>{t("level_records.best_label", {value: formatRecordValue(item.best)})}</span>
                                    </div>
                                ) : null}
                            </div>
                        </div>
                    );
                })}
            </div>
        </aside>
    );
}
