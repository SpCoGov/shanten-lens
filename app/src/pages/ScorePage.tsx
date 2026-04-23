import React from "react";
import {useTranslation} from "react-i18next";
import Modal from "../components/Modal";
import AmuletCard from "../components/AmuletCard";
import {getAllRegisteredAmuletRules} from "../lib/amuletRuleRegistry";
import type {EffectItem} from "../lib/gamestate";
import {useRegistry} from "../lib/registryStore";
import {
    calculateCurrentPoint,
    clampExtraExecutionCount,
    clampExecutionCount,
    clampManualExtraTriggerCount,
    computeBaseScore,
    DEFAULT_RULE_CONFIG,
    PRESET_AMULET_RULES,
    fixed2ToString,
    formatFixed2,
    parseStoredDataFromItem,
    parseStoredDataListFromItem,
    parseFixed2,
    parseTargetPointValue,
    projectFuturePoints,
    resolveAmuletRule,
    getRegisteredAmuletRuleExact,
    type AmuletRuleConfig,
    type EffectTarget,
    type ResolvedAmuletRule,
} from "../lib/scoreEngine";
import {buildDoraCountByTile} from "../lib/tileHighlights";

type CustomRuleMap = Record<string, Partial<AmuletRuleConfig>>;
type LevelTargetEntry = { label: string; level: number; target: string };
type TileScoreEntry = { tile: string; score: bigint };
type DriverFilter = "all" | "code" | "config" | "unconfigured";

function cloneResolvedRules(rules: ResolvedAmuletRule[]): ResolvedAmuletRule[] {
    return rules.map((rule) => ({
        ...rule,
        dataRawList: [...rule.dataRawList],
    }));
}

function toggleGroupKey(list: string[], key: string): string[] {
    return list.includes(key) ? list.filter((item) => item !== key) : [...list, key];
}

const RULES_STORAGE_KEY = "shanten:point-rules:v1";
const FAN_STORAGE_KEY = "shanten:point-fan:v1";
const WIN_COUNT_STORAGE_KEY = "shanten:point-win-count:v1";

const LEVEL_TARGETS: Record<string, string> = {
    "1-1": "200",
    "1-2": "400",
    "1-3": "700",
    "2-1": "1000",
    "2-2": "1500",
    "2-3": "2100",
    "3-1": "3000",
    "3-2": "4000",
    "3-3": "5200",
    "4-1": "7000",
    "4-2": "9800",
    "4-3": "15000",
    "5-1": "22500",
    "5-2": "33300",
    "5-3": "50000",
    "Ex1": "1000000",
    "Ex2": "12000000",
    "Ex3": "90750000",
    "Ex4": "8.11亿",
    "Ex5": "84.28亿",
    "Ex6": "1003.54亿",
    "Ex7": "1.35兆",
    "Ex8": "20.56兆",
    "Ex9": "348.31兆",
    "Ex10": "6542.41兆",
    "Ex11": "13.55京",
    "Ex12": "308.54京",
    "Ex13": "7681.69京",
    "Ex14": "20.84垓",
    "Ex15": "664.10垓",
    "Ex16": "2.46秭",
    "Ex17": "105.10秭",
    "Ex18": "5154.00秭",
    "Ex19": "28.82穰",
    "Ex20": "1830.00穰",
    "Ex21": "13.14沟",
    "Ex22": "1066.00沟",
    "Ex23": "9.72涧",
    "Ex24": "994.10涧",
    "Ex25": "11.38正",
    "Ex26": "1456.00正",
    "Ex27": "20.79载",
    "Ex28": "3306.0载",
    "Ex29": "58.49极",
    "Ex30": "1.25万极",
    "Ex31": "300.70万极",
    "Ex32": "8.71亿极",
    "Ex33": "2792.00亿极",
    "Ex34": "108.20兆极",
    "Ex35": "4.63京极",
    "Ex36": "2398.00京极",
    "Ex37": "136.90垓极",
    "Ex38": "9.41秭极",
    "Ex39": "7147.00秭极",
    "Ex40": "708.60穰极",
    "Ex41": "91.69沟极",
    "Ex42": "15.46涧极",
    "Ex43": "3.40正极",
    "Ex44": "9734.00正极",
    "Ex45": "3627.00载极",
    "Ex46": "1759.00极极",
    "Ex47": "1109.00万极极",
    "Ex48": "910.80亿极极",
    "Ex49": "972.20兆极极",
    "Ex50": "1349.00京极极",
    "Ex51": "2811.00垓极极",
    "Ex52": "8787.00秭极极",
    "Ex53": "4.12沟极极",
    "Ex54": "28.99涧极极",
    "Ex55": "306.00正极极",
    "Ex56": "4844.00载极极",
    "Ex57": "11.5万极极极",
    "Ex58": "409.9亿极极极",
    "Ex59": "2.19京极极极",
    "Ex60": "175.50垓极极极",
    "Ex61": "2.11穰极极极",
    "Ex62": "380.79沟极极极",
    "Ex63": "10.30正极极极",
    "Ex64": "4180.00载极极极",
    "Ex65": "254.4万极极极极",
    "Ex66": "23.23兆极极极极",
    "Ex67": "3.18垓极极极极",
    "Ex68": "6538.00秭极极极极",
    "Ex69": "2015.00沟极极极极",
    "Ex70": "931.40正极极极极",
    "Ex71": "645.80极极极极极",
    "Ex72": "671.70亿极极极极极",
    "Ex73": "1047.00京极极极极极",
    "Ex74": "2452.00秭极极极极极",
    "Ex75": "8608.00沟极极极极极",
    "Ex76": "4.53载极极极极极",
    "Ex77": "35.79万极极极极极极",
    "Ex78": "424.00兆极极极极极极",
    "Ex79": "5.70E+310",
    "Ex80": "5.70E+318",
    "Ex81": "5.70E+326",
    "Ex82": "5.70E+334",
    "Ex83": "5.70E+342",
    "Ex84": "5.70E+350",
    "Ex85": "5.70E+359",
    "Ex86": "5.70E+368",
    "Ex87": "5.70E+377",
    "Ex88": "5.70E+386",
    "Ex89": "5.70E+395",
    "Ex90": "5.70E+405",
    "Ex91": "5.70E+415",
    "Ex92": "5.70E+425",
    "Ex93": "5.70E+435",
    "Ex94": "5.70E+445",
    "Ex95": "5.70E+455",
    "Ex96": "5.70E+466",
    "Ex97": "5.70E+477",
    "Ex98": "5.70E+488",
    "Ex99": "5.70E+499",
    "Ex100": "9.99E+510",
    "Ex101": "9.99E+522",
    "Ex102": "9.99E+534",
    "Ex103": "9.99E+546",
    "Ex104": "9.99E+558",
    "Ex105": "9.99E+571",
    "Ex106": "9.99E+584",
    "Ex107": "9.99E+597",
    "Ex108": "9.99E+611",
    "Ex109": "9.99E+625",
    "Ex110": "9.99E+639",
    "Ex111": "9.99E+654",
    "Ex112": "9.99E+669",
    "Ex113": "9.99E+684",
    "Ex114": "9.99E+699",
    "Ex115": "9.99E+715",
    "Ex116": "9.99E+731",
    "Ex117": "9.99E+748",
    "Ex118": "9.99E+765",
    "Ex119": "9.99E+783",
    "Ex120": "9.99E+801",
    "Ex121": "9.999E+820",
    "Ex122": "9.999E+839",
    "Ex123": "9.999E+859",
    "Ex124": "9.999E+879",
    "Ex125": "9.999E+900",
    "Ex126": "9.999E+921",
    "Ex127": "9.999E+943",
    "Ex128": "9.999E+966",
    "Ex129": "9.999E+990",
    "Ex130": "9.999E+1015",
    "Ex131": "9.999E+1041",
    "Ex132": "9.999E+1068",
    "Ex133": "9.999E+1096",
    "Ex134": "9.999E+1125",
    "Ex135": "9.999E+1155",
    "Ex136": "9.999E+1186",
    "Ex137": "9.999E+1218",
    "Ex138": "9.999E+1251",
    "Ex139": "9.999E+1285",
    "Ex140": "9.999E+1320",
    "Ex141": "9.999E+1356",
    "Ex142": "9.999E+1393",
    "Ex143": "9.999E+1431",
    "Ex144": "9.999E+1470",
    "Ex145": "9.999E+1510",
    "Ex146": "9.999E+1551",
    "Ex147": "9.999E+1593",
    "Ex148": "9.999E+1636",
    "Ex149": "9.999E+1680",
    "Ex150": "9.999E+1725",
    "Ex151": "9.999E+1771",
    "Ex152": "9.999E+1818",
    "Ex153": "9.999E+1866",
    "Ex154": "9.999E+1915",
    "Ex155": "9.999E+1965",
    "Ex156": "9.999E+2016",
    "Ex157": "9.999E+2068",
    "Ex158": "9.999E+2121",
    "Ex159": "9.999E+2175",
    "Ex160": "9.999E+2230",
    "Ex161": "9.999E+2286",
    "Ex162": "9.999E+2343",
    "Ex163": "9.999E+2401",
    "Ex164": "9.999E+2460",
};

const FUTURE_POINT_TARGETS: Record<number, string> = {
    1165: "9999000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    1166: "99990000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    1167: "9999000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    1168: "29997000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
};

function loadJsonObject<T>(key: string, fallback: T): T {
    try {
        const raw = localStorage.getItem(key);
        if (!raw) return fallback;
        return JSON.parse(raw) as T;
    } catch {
        return fallback;
    }
}

function usePersistentState<T>(key: string, fallback: T) {
    const [value, setValue] = React.useState<T>(() => loadJsonObject(key, fallback));

    React.useEffect(() => {
        try {
            localStorage.setItem(key, JSON.stringify(value));
        } catch {
        }
    }, [key, value]);

    return [value, setValue] as const;
}

function getRuleKey(item: EffectItem) {
    return `${item.uid}:${item.id}`;
}

function parseLevelKeyToId(key: string): number | null {
    const normal = key.match(/^(\d+)-(\d+)$/);
    if (normal) return Number(`${normal[1]}0${normal[2]}`);
    const ex = key.match(/^Ex(\d+)$/i);
    if (ex) return 1000 + Number(ex[1]);
    return null;
}

function formatLevelIdToLabel(level: number): string {
    if (level >= 1001) return `Ex${level - 1000}`;
    const chapter = Math.trunc(level / 100);
    const stage = level % 10;
    if (chapter > 0 && stage > 0) return `${chapter}-${stage}`;
    return String(level);
}

function formatTargetText(target: string | undefined): string {
    if (!target) return target ?? "";
    const parsed = parseTargetPointValue(target);
    return parsed == null ? target : formatFixed2(parsed);
}

function getTileScoreGroup(tile: string): "m" | "p" | "s" | "z" | "other" {
    const normalized = String(tile ?? "").trim();
    if (/^[0-9][mps]$/.test(normalized)) return normalized[1] as "m" | "p" | "s";
    if (/^[1-7]z$/.test(normalized)) return "z";
    return "other";
}

function getTileSortValue(tile: string): number {
    const normalized = String(tile ?? "").trim();
    if (/^[0-9][mps]$/.test(normalized) || /^[1-7]z$/.test(normalized)) {
        const value = Number(normalized[0]);
        return value === 0 ? 5 : value;
    }
    return Number.MAX_SAFE_INTEGER;
}

function normalizeTileCode(tile: string): string | null {
    const normalized = String(tile ?? "").trim();
    if (/^[0-9][mps]$/.test(normalized)) return normalized;
    if (/^[mps][0-9]$/.test(normalized)) return `${normalized[1]}${normalized[0]}`;
    if (/^[1-7]z$/.test(normalized)) return normalized;
    if (/^z[1-7]$/.test(normalized)) return `${normalized[1]}z`;
    return null;
}

function isPinzuTile(tile: string | undefined): boolean {
    return /^[0-9]p$/.test(String(tile ?? "").trim());
}

const BASE_LEVEL_TARGETS: LevelTargetEntry[] = Object.entries(LEVEL_TARGETS)
    .map(([label, target]) => {
        const level = parseLevelKeyToId(label);
        if (level == null) return null;
        return {label, level, target};
    })
    .filter((item): item is LevelTargetEntry => item != null);

const ORDERED_LEVEL_TARGETS: LevelTargetEntry[] = Array.from(
    new Map<number, LevelTargetEntry>([
        ...BASE_LEVEL_TARGETS.map((item) => [item.level, item] as const),
        ...Object.entries(FUTURE_POINT_TARGETS).map(([levelText, target]) => {
            const level = Number(levelText);
            return [level, {label: formatLevelIdToLabel(level), level, target}] as const;
        }),
    ]).values(),
).sort((left, right) => left.level - right.level);

const LEVEL_TARGETS_BY_ID = ORDERED_LEVEL_TARGETS.reduce<Record<number, string>>((acc, item) => {
    acc[item.level] = item.target;
    return acc;
}, {});

const LEVEL_LABELS_BY_ID = ORDERED_LEVEL_TARGETS.reduce<Record<number, string>>((acc, item) => {
    acc[item.level] = item.label;
    return acc;
}, {});

function safeDisplayStoredData(raw: string) {
    try {
        return formatFixed2(BigInt(raw || "0"));
    } catch {
        return "0";
    }
}

function safeDisplayStoredDataList(rawList: string[]) {
    return rawList.map((raw, index) => ({
        index,
        raw,
        rawDisplay: safeDisplayStoredData(raw),
        display: safeDisplayStoredData(raw),
    }));
}

function EffectTargetOptions({t}: { t: (key: string) => string }) {
    return (
        <>
            <option value="none">{t("score.effect_target_none")}</option>
            <option value="score">{t("score.effect_target_score")}</option>
            <option value="fan">{t("score.effect_target_fan")}</option>
        </>
    );
}

export default function ScorePage({
                                      amulets,
                                      handTileIds,
                                      deckMap,
                                      tileScoreMap,
                                      doraTileIds,
                                      tianDoraTiles,
                                      level,
                                      currentPoint,
                                      currentTargetPoint,
                                  }: {
    amulets: EffectItem[];
    handTileIds: number[];
    deckMap: Map<number, string>;
    tileScoreMap: Record<string, string>;
    doraTileIds: number[];
    tianDoraTiles: string[];
    level: number;
    currentPoint?: string;
    currentTargetPoint?: string;
}) {
    const {t} = useTranslation();
    const [fanText, setFanText] = usePersistentState<string>(FAN_STORAGE_KEY, "1");
    const [winCountText, setWinCountText] = usePersistentState<string>(WIN_COUNT_STORAGE_KEY, "1");
    const [customRules, setCustomRules] = usePersistentState<CustomRuleMap>(RULES_STORAGE_KEY, {});
    const [editingKey, setEditingKey] = React.useState<string | null>(null);
    const [showTileScores, setShowTileScores] = React.useState(false);
    const [showRuleOverview, setShowRuleOverview] = React.useState(false);
    const [selectedFutureLevel, setSelectedFutureLevel] = React.useState<number | null>(null);
    const [selectedExecExplainIndex, setSelectedExecExplainIndex] = React.useState<number | null>(null);
    const [expandedFutureGroups, setExpandedFutureGroups] = React.useState<string[]>([]);
    const [ruleOverviewRarityFilter, setRuleOverviewRarityFilter] = React.useState<string>("all");
    const [ruleOverviewDriverFilter, setRuleOverviewDriverFilter] = React.useState<DriverFilter>("all");
    const [draftRule, setDraftRule] = React.useState<AmuletRuleConfig>(DEFAULT_RULE_CONFIG);
    const [manualExtraTriggerInput, setManualExtraTriggerInput] = React.useState("0");
    const registry = useRegistry();

    const baseFan = React.useMemo(() => {
        try {
            return parseFixed2(fanText || "0");
        } catch {
            return 0n;
        }
    }, [fanText]);

    const winCount = React.useMemo(() => {
        const n = Number.parseInt(String(winCountText ?? "").trim(), 10);
        if (!Number.isFinite(n)) return 1;
        return Math.max(1, Math.min(999, Math.trunc(n)));
    }, [winCountText]);

    const baseScore = React.useMemo(() => computeBaseScore(handTileIds, deckMap, tileScoreMap), [handTileIds, deckMap, tileScoreMap]);
    const hasPinzuInHand = React.useMemo(
        () => handTileIds.some((tileId) => isPinzuTile(deckMap.get(tileId))),
        [handTileIds, deckMap],
    );

    const rules = React.useMemo(() => {
        return amulets.map((item) => resolveAmuletRule(item, customRules[getRuleKey(item)]));
    }, [amulets, customRules]);

    const tileScoreGroups = React.useMemo(() => {
        const grouped: Record<"m" | "p" | "s" | "z" | "other", TileScoreEntry[]> = {
            m: [],
            p: [],
            s: [],
            z: [],
            other: [],
        };

        Object.entries(tileScoreMap ?? {}).forEach(([tile, score]) => {
            let parsed = 0n;
            try {
                parsed = parseFixed2(score ?? "0");
            } catch {
                parsed = 0n;
            }
            grouped[getTileScoreGroup(tile)].push({tile, score: parsed});
        });

        (Object.keys(grouped) as Array<keyof typeof grouped>).forEach((key) => {
            grouped[key].sort((left, right) => {
                const valueDiff = getTileSortValue(left.tile) - getTileSortValue(right.tile);
                if (valueDiff !== 0) return valueDiff;
                return left.tile.localeCompare(right.tile);
            });
        });

        return grouped;
    }, [tileScoreMap]);

    const tianDoraTileSet = React.useMemo(
        () => new Set((tianDoraTiles ?? []).map((tile) => String(tile ?? "").trim()).filter(Boolean)),
        [tianDoraTiles],
    );

    const doraCountByTile = React.useMemo(
        () => buildDoraCountByTile(deckMap, doraTileIds),
        [deckMap, doraTileIds],
    );

    const soulTileCount = 14;

    const currentResult = React.useMemo(() => {
        const runtime = {hasPinzuInHand, soulTileCount};
        return calculateCurrentPoint(baseScore, baseFan, level, cloneResolvedRules(rules), runtime);
    }, [baseScore, baseFan, level, rules, hasPinzuInHand, soulTileCount]);

    const currentLevelLabel = React.useMemo(
        () => LEVEL_LABELS_BY_ID[level] ?? formatLevelIdToLabel(level || 0),
        [level],
    );

    const futureProjections = React.useMemo(() => {
        const startIndex = ORDERED_LEVEL_TARGETS.findIndex((item) => item.level === level);
        if (startIndex < 0) return [];
        const runtime = {hasPinzuInHand, soulTileCount: 14};
        const seededRules = cloneResolvedRules(rules);
        let seededResult = calculateCurrentPoint(baseScore, baseFan, level, seededRules, runtime);
        for (let winIndex = 1; winIndex < winCount; winIndex += 1) {
            seededResult = calculateCurrentPoint(baseScore, baseFan, level, seededRules, runtime);
        }
        return projectFuturePoints(
            level,
            ORDERED_LEVEL_TARGETS.slice(startIndex + 1).map((item) => ({
                level: item.level,
                target: LEVEL_TARGETS_BY_ID[item.level],
            })),
            seededResult,
            seededRules,
            baseScore,
            baseFan,
            winCount,
            runtime,
        );
    }, [level, rules, baseScore, baseFan, winCount, hasPinzuInHand]);

    const projectionMetaByLevel = React.useMemo(
        () => new Map(ORDERED_LEVEL_TARGETS.map((item) => [item.level, item] as const)),
        [],
    );
    const selectedFutureProjection = React.useMemo(
        () => futureProjections.find((projection) => projection.level === selectedFutureLevel) ?? null,
        [futureProjections, selectedFutureLevel],
    );
    const selectedExecExplain = React.useMemo(
        () => (selectedExecExplainIndex == null ? null : currentResult.perAmulet[selectedExecExplainIndex] ?? null),
        [currentResult.perAmulet, selectedExecExplainIndex],
    );
    const displayedFutureItems = React.useMemo(() => {
        const items: Array<
            | { type: "projection"; projection: typeof futureProjections[number] }
            | {
            type: "collapsed";
            groupKey: string;
            hiddenCount: number;
            startLevel: string;
            endLevel: string;
            expanded: boolean;
            hiddenProjections: typeof futureProjections;
        }
        > = [];
        let index = 0;
        while (index < futureProjections.length) {
            const projection = futureProjections[index];
            if (!projection || projection.reached !== true) {
                if (projection) items.push({type: "projection", projection});
                index += 1;
                continue;
            }

            let end = index;
            while (end + 1 < futureProjections.length && futureProjections[end + 1]?.reached === true) {
                end += 1;
            }

            const run = futureProjections.slice(index, end + 1);
            if (run.length <= 2) {
                run.forEach((entry) => items.push({type: "projection", projection: entry}));
            } else {
                items.push({type: "projection", projection: run[0]!});
                const hiddenProjections = run.slice(1, -1);
                const groupKey = `${run[0]!.level}:${run[run.length - 1]!.level}`;
                items.push({
                    type: "collapsed", groupKey,
                    hiddenCount: hiddenProjections.length,
                    startLevel: projectionMetaByLevel.get(hiddenProjections[0]!.level)?.label ?? formatLevelIdToLabel(hiddenProjections[0]!.level),
                    endLevel: projectionMetaByLevel.get(hiddenProjections[hiddenProjections.length - 1]!.level)?.label ?? formatLevelIdToLabel(hiddenProjections[hiddenProjections.length - 1]!.level),
                    expanded: expandedFutureGroups.includes(groupKey),
                    hiddenProjections,
                });
                items.push({type: "projection", projection: run[run.length - 1]!});
            }
            index = end + 1;
        }
        return items;
    }, [expandedFutureGroups, futureProjections, projectionMetaByLevel]);
    const chainBreakIndices = React.useMemo(() => {
        const breaks = new Set<number>();
        for (let index = 1; index < rules.length; index += 1) {
            const prev = rules[index - 1];
            const current = rules[index];
            const currentApplied = (currentResult.perAmulet[index]?.effectApplications ?? 0) > 0;
            if (prev?.hasTransmissionSeal && current?.hasTransmissionSeal && !currentApplied) {
                breaks.add(index);
            }
        }
        return breaks;
    }, [currentResult.perAmulet, rules]);

    const tileScoreSections = React.useMemo(
        () => ([
            {key: "m", title: t("score.tile_group_manzu"), entries: tileScoreGroups.m},
            {key: "p", title: t("score.tile_group_pinzu"), entries: tileScoreGroups.p},
            {key: "s", title: t("score.tile_group_souzu"), entries: tileScoreGroups.s},
            {key: "z", title: t("score.tile_group_honor"), entries: tileScoreGroups.z},
            {key: "other", title: t("score.tile_group_other"), entries: tileScoreGroups.other},
        ]).filter((section) => section.entries.length > 0),
        [t, tileScoreGroups],
    );

    const actualPoint = React.useMemo(() => {
        try {
            return parseFixed2(currentPoint ?? "0");
        } catch {
            return 0n;
        }
    }, [currentPoint]);

    const actualTarget = React.useMemo(() => {
        try {
            return parseFixed2(currentTargetPoint ?? "0");
        } catch {
            return 0n;
        }
    }, [currentTargetPoint]);
    const totalCurrentPoint = React.useMemo(
        () => currentResult.finalPoint * BigInt(winCount),
        [currentResult.finalPoint, winCount],
    );

    const editingItem = React.useMemo(
        () => amulets.find((item) => getRuleKey(item) === editingKey) ?? null,
        [amulets, editingKey],
    );
    const editingRegId = React.useMemo(() => (editingItem ? editingItem.id : null), [editingItem]);
    const editingStoredDataList = React.useMemo(
        () => (editingItem ? safeDisplayStoredDataList(parseStoredDataListFromItem(editingItem)) : []),
        [editingItem],
    );
    const isEditingCodeDriven = React.useMemo(
        () => (editingRegId == null ? false : !!getRegisteredAmuletRuleExact(editingRegId)),
        [editingRegId],
    );

    const openEditor = React.useCallback((item: EffectItem) => {
        const merged = resolveAmuletRule(item, customRules[getRuleKey(item)]);
        setDraftRule({
            dataRaw: merged.dataRaw,
            executions: merged.executions,
            extraExecutions: merged.extraExecutions,
            manualExtraTriggers: merged.manualExtraTriggers,
            activeOnWin: merged.activeOnWin,
            growthAfterRound: merged.growthAfterRound,
            effectTarget: merged.effectTarget,
            effectFormula: merged.effectFormula,
            growthFormula: merged.growthFormula,
            triggerGrowthFormula: merged.triggerGrowthFormula,
            winGrowthFormula: merged.winGrowthFormula,
            note: merged.note ?? "",
        });
        setManualExtraTriggerInput(String(merged.manualExtraTriggers ?? 0));
        setEditingKey(getRuleKey(item));
    }, [customRules]);

    const saveEditor = React.useCallback(() => {
        if (!editingItem || !editingKey) return;
        const manualExtraTriggers = clampManualExtraTriggerCount(manualExtraTriggerInput);
        setCustomRules((prev) => ({
            ...prev,
            [editingKey]: {
                dataRaw: parseStoredDataFromItem(editingItem),
                executions: clampExecutionCount(draftRule.executions),
                extraExecutions: clampExtraExecutionCount(draftRule.extraExecutions),
                manualExtraTriggers,
                activeOnWin: draftRule.activeOnWin,
                growthAfterRound: draftRule.growthAfterRound,
                effectTarget: draftRule.effectTarget,
                effectFormula: draftRule.effectFormula.trim(),
                growthFormula: draftRule.growthFormula.trim() || "data",
                triggerGrowthFormula: draftRule.triggerGrowthFormula.trim(),
                winGrowthFormula: draftRule.winGrowthFormula.trim(),
                note: draftRule.note?.trim() ?? "",
            },
        }));
        setEditingKey(null);
    }, [draftRule, editingItem, editingKey, manualExtraTriggerInput, setCustomRules]);

    const resetEditor = React.useCallback(() => {
        if (!editingKey) return;
        setCustomRules((prev) => {
            const next = {...prev};
            delete next[editingKey];
            return next;
        });
        setEditingKey(null);
    }, [editingKey, setCustomRules]);

    const currentReached = actualTarget > 0n ? totalCurrentPoint >= actualTarget : null;
    const codeRuleIdSet = React.useMemo(
        () => new Set(Object.keys(getAllRegisteredAmuletRules()).map((id) => Number(id)).filter((id) => Number.isFinite(id))),
        [],
    );
    const presetRuleIdSet = React.useMemo(
        () => new Set(Object.keys(PRESET_AMULET_RULES).map((id) => Number(id)).filter((id) => Number.isFinite(id))),
        [],
    );
    const hasCustomConfigByAmuletId = React.useMemo(() => {
        const out = new Set<number>();
        Object.keys(customRules).forEach((key) => {
            const sep = key.lastIndexOf(":");
            if (sep < 0) return;
            const id = Number(key.slice(sep + 1));
            if (Number.isFinite(id)) out.add(id);
        });
        return out;
    }, [customRules]);
    const registryAmuletOverview = React.useMemo(
        () => {
            const allIds = new Set<number>();
            (registry.amulets ?? []).forEach((amulet) => {
                const baseId = amulet.id * 10;
                allIds.add(baseId);
                allIds.add(baseId + 1);
            });
            codeRuleIdSet.forEach((id) => allIds.add(id));
            presetRuleIdSet.forEach((id) => allIds.add(id));
            hasCustomConfigByAmuletId.forEach((id) => allIds.add(id));

            return Array.from(allIds)
                .filter((id) => Number.isFinite(id))
                .sort((a, b) => a - b)
                .map((id) => {
                    const regId = Math.floor(id / 10);
                    const rarity = registry.amuletById.get(regId)?.rarity ?? "GREEN";
                    const isCodeDriven = codeRuleIdSet.has(id);
                    const hasPresetConfig = presetRuleIdSet.has(id);
                    const hasCustomConfig = hasCustomConfigByAmuletId.has(id);
                    const isConfigured = isCodeDriven || hasPresetConfig || hasCustomConfig;
                    const driverType: DriverFilter = isCodeDriven ? "code" : isConfigured ? "config" : "unconfigured";
                    return {
                        id,
                        rarity,
                        isCodeDriven,
                        isConfigured,
                        hasCustomConfig,
                        driverType,
                        driverLabel:
                            driverType === "code"
                                ? t("score.code_driven")
                                : driverType === "config"
                                    ? t("score.config_driven")
                                    : t("score.unconfigured"),
                    };
                });
        },
        [codeRuleIdSet, hasCustomConfigByAmuletId, presetRuleIdSet, registry.amuletById, registry.amulets, t],
    );
    const rarityOptions = React.useMemo(
        () => ["all", ...Array.from(new Set((registry.amulets ?? []).map((a) => a.rarity))).sort()],
        [registry.amulets],
    );
    const filteredRegistryAmuletOverview = React.useMemo(() => {
        return registryAmuletOverview.filter((entry) => {
            const rarityOk = ruleOverviewRarityFilter === "all" || entry.rarity === ruleOverviewRarityFilter;
            const driverOk = ruleOverviewDriverFilter === "all" || entry.driverType === ruleOverviewDriverFilter;
            return rarityOk && driverOk;
        });
    }, [registryAmuletOverview, ruleOverviewDriverFilter, ruleOverviewRarityFilter]);
    const removeConfigForAmuletId = React.useCallback((amuletId: number) => {
        setCustomRules((prev) => {
            const next: CustomRuleMap = {};
            Object.entries(prev).forEach(([key, value]) => {
                const sep = key.lastIndexOf(":");
                const id = sep < 0 ? NaN : Number(key.slice(sep + 1));
                if (id === amuletId) return;
                next[key] = value;
            });
            return next;
        });
    }, [setCustomRules]);

    return (
        <div className="settings-wrap wide-page" style={{paddingBlock: 16}}>
            <h2 className="title">{t("score.title")}</h2>

            <div className="page-stack">
                <section className="panel">
                    <div style={{display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center", marginBottom: 10, flexWrap: "wrap"}}>
                        <div className="panel-title" style={{marginBottom: 0}}>{t("score.live_point_title")}</div>
                        <button className="nav-btn" onClick={() => setShowTileScores(true)}>
                            {t("score.view_tile_scores")}
                        </button>
                    </div>
                    <div className="responsive-two-col" style={{alignItems: "start"}}>
                        <div className="rows">
                            <div className="row">
                                <label>{t("score.current_level")}</label>
                                <div className="badge">{currentLevelLabel}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.base_fan")}</label>
                                <div>
                                    <input
                                        className="form-input"
                                        value={fanText}
                                        onChange={(e) => setFanText(e.target.value)}
                                        placeholder="1"
                                    />
                                </div>
                            </div>
                            <div className="row">
                                <label>{t("score.win_count")}</label>
                                <div>
                                    <input
                                        className="form-input"
                                        value={winCountText}
                                        onChange={(e) => setWinCountText(e.target.value)}
                                        placeholder="1"
                                    />
                                </div>
                            </div>
                            <div className="row">
                                <label>{t("score.base_score")}</label>
                                <div className="badge">{formatFixed2(currentResult.baseScore)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.final_score")}</label>
                                <div className="badge">{formatFixed2(currentResult.finalScore)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.final_fan")}</label>
                                <div className="badge">{formatFixed2(currentResult.finalFan)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.final_point")}</label>
                                <div className="badge ok">{formatFixed2(currentResult.finalPoint)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.total_point")}</label>
                                <div className="badge ok">{formatFixed2(totalCurrentPoint)}</div>
                            </div>
                        </div>

                        <div className="rows">
                            <div className="row">
                                <label>{t("score.actual_point")}</label>
                                <div className="badge">{formatFixed2(actualPoint)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.current_target")}</label>
                                <div className="badge">{formatFixed2(actualTarget)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.current_target_result")}</label>
                                <div className={`badge ${currentReached ? "ok" : "down"}`}>
                                    {currentReached == null ? t("score.not_set") : currentReached ? t("score.reached") : t("score.not_reached")}
                                </div>
                            </div>
                        </div>
                    </div>
                </section>

                <section className="panel">
                    <div style={{display: "flex", justifyContent: "space-between", gap: 12, alignItems: "center", marginBottom: 6, flexWrap: "wrap"}}>
                        <div className="panel-title" style={{marginBottom: 0}}>{t("score.current_amulets")}</div>
                        <button className="nav-btn" onClick={() => setShowRuleOverview(true)}>
                            {t("score.view_rule_overview")}
                        </button>
                    </div>
                    {amulets.length === 0 ? (
                        <div className="hint">{t("noAmulet")}</div>
                    ) : (
                        <div style={{display: "flex", gap: 10, overflowX: "auto", padding: "8px 4px"}}>
                            {rules.map((rule, index) => {
                                const effectApplicationCount =
                                    currentResult.perAmulet[index]?.effectApplications ??
                                    currentResult.perAmulet[index]?.activations ??
                                    (rule.executions + (rule.extraExecutions ?? 0));
                                const manualExtraTriggerCount =
                                    currentResult.perAmulet[index]?.manualExtraTriggers ??
                                    rule.manualExtraTriggers ??
                                    0;
                                const isCodeDriven = !!getRegisteredAmuletRuleExact(rule.regId);
                                const hasPresetConfig = !!PRESET_AMULET_RULES[rule.regId];
                                const hasCustomConfig = !!customRules[getRuleKey(rule.item)];
                                const isUnconfigured =
                                    !isCodeDriven &&
                                    !hasPresetConfig &&
                                    !hasCustomConfig;
                                const isChainBreak = chainBreakIndices.has(index);
                                const key = `${rule.item.uid}-${rule.item.id}-${index}`;

                                return (
                                    <div
                                        key={key}
                                        style={{
                                            flex: "0 0 auto", display: "grid", gap: 10,
                                            justifyItems: "center", alignContent: "start", minWidth: 132,
                                        }}
                                    >
                                        <div
                                            style={{
                                                position: "relative", display: "grid", placeItems: "center", width: Math.round(160 * 0.44),
                                                minHeight: Math.round(220 * 0.44),
                                            }}
                                        >
                                            <button
                                                className="target-card-button"
                                                onClick={() => openEditor(rule.item)}
                                                style={{
                                                    width: "auto", background: "transparent", border: "none", padding: 0,
                                                }}
                                                title={isCodeDriven ? t("score.code_driven") : t("score.edit_amulet")}
                                            >
                                                <AmuletCard item={rule.item} scale={0.44}/>
                                            </button>
                                            {isUnconfigured ? (
                                                <div
                                                    className="badge down"
                                                    style={{
                                                        position: "absolute", top: -8,
                                                        left: -8,
                                                        paddingInline: 8,
                                                    }}
                                                >
                                                    {t("score.unconfigured")}
                                                </div>
                                            ) : null}
                                            {manualExtraTriggerCount !== 0 ? (
                                                <div
                                                    className={`badge ${manualExtraTriggerCount > 0 ? "ok" : "down"}`}
                                                    style={{
                                                        position: "absolute", top: -8,
                                                        right: -8,
                                                        paddingInline: 8,
                                                        minWidth: 34,
                                                        justifyContent: "center", textAlign: "center",
                                                    }}
                                                    title={t("score.manual_extra_triggers_label")}
                                                >
                                                    {manualExtraTriggerCount > 0 ? `+${manualExtraTriggerCount}` : String(manualExtraTriggerCount)}
                                                </div>
                                            ) : null}
                                        </div>
                                        <button
                                            type="button"
                                            className="badge"
                                            onClick={() => setSelectedExecExplainIndex(index)}
                                            style={{
                                                whiteSpace: "nowrap", lineHeight: 1,
                                                minHeight: 38,
                                                display: "inline-flex", alignItems: "center", justifyContent: "center", paddingInline: 14,
                                                borderColor: isChainBreak ? "var(--badge-down-border)" : undefined,
                                                background: isChainBreak ? "var(--badge-down-bg)" : undefined,
                                                color: isChainBreak ? "var(--badge-down-fg)" : undefined,
                                                cursor: "pointer",
                                            }}
                                            title={isChainBreak ? t("score.transmission_chain_break") : undefined}
                                        >
                                            {t("score.exec_count", {count: effectApplicationCount})}
                                        </button>
                                    </div>
                                );
                            })}
                        </div>
                    )}
                </section>

                <section className="panel">
                    <div className="panel-title">{t("score.future_projection_title")}</div>
                    <div style={{display: "grid", gap: 10}}>
                        {displayedFutureItems.map((item, itemIndex) => {
                            if (item.type === "collapsed") {
                                return (
                                    <div
                                        key={`collapsed:${item.startLevel}:${item.endLevel}:${itemIndex}`}
                                        className="target-card future-collapsed-card"
                                    >
                                        <div className="target-card-main" style={{gridTemplateColumns: "minmax(0, 1fr)"}}>
                                            <div className="target-card-copy future-collapsed-copy">
                                                <div className="future-collapsed-title">
                                                    {t("score.future_projection_collapsed_reached", {
                                                        count: item.hiddenCount,
                                                        start: item.startLevel,
                                                        end: item.endLevel,
                                                    })}
                                                </div>
                                                <div className="hint future-collapsed-meta">
                                                    {item.startLevel} - {item.endLevel}
                                                </div>
                                            </div>
                                        </div>
                                        <div className="target-card-actions">
                                            <span className="badge ok">{t("score.reached")}</span>
                                            <button
                                                type="button"
                                                className="nav-btn future-collapsed-toggle"
                                                onClick={() => setExpandedFutureGroups((prev) => toggleGroupKey(prev, item.groupKey))}
                                            >
                                                <span aria-hidden="true">{item.expanded ? "▾" : "▸"}</span>
                                                <span>
                                                    {item.expanded
                                                        ? t("score.future_projection_collapse")
                                                        : t("score.future_projection_expand")}
                                                </span>
                                            </button>
                                        </div>
                                        {item.expanded ? (
                                            <div className="future-collapsed-list">
                                                {item.hiddenProjections.map((projection) => {
                                                    const meta = projectionMetaByLevel.get(projection.level);
                                                    const levelLabel = meta?.label ?? formatLevelIdToLabel(projection.level);
                                                    const targetText = meta?.target;
                                                    return (
                                                        <button
                                                            key={`expanded:${projection.level}`}
                                                            className="target-card target-card-button future-collapsed-item"
                                                            onClick={() => setSelectedFutureLevel(projection.level)}
                                                        >
                                                            <div className="target-card-main" style={{gridTemplateColumns: "minmax(0, 1fr)"}}>
                                                                <div className="target-card-copy">
                                                                    <div style={{fontWeight: 700}}>
                                                                        {t("score.future_level_title", {level: levelLabel})}
                                                                    </div>
                                                                    <div className="future-projection-metrics">
                                                                        <div className="future-projection-metric">
                                                                            <span className="future-projection-label">{t("score.projected_point")}</span>
                                                                            <span>{formatFixed2(projection.point)}</span>
                                                                        </div>
                                                                        <div className="future-projection-metric">
                                                                            <span className="future-projection-label">{t("score.projected_total_point")}</span>
                                                                            <span>{formatFixed2(projection.totalPoint)}</span>
                                                                        </div>
                                                                        <div className="future-projection-metric">
                                                                            <span className="future-projection-label">{t("score.projected_score")} / {t("score.projected_fan")}</span>
                                                                            <span>{formatFixed2(projection.score)} / {formatFixed2(projection.fan)}</span>
                                                                        </div>
                                                                    </div>
                                                                </div>
                                                            </div>
                                                            <div className="target-card-actions">
                                                                <span className="badge">
                                                                    {t("score.future_target_label")}: {targetText == null ? t("score.future_target_placeholder") : formatTargetText(targetText)}
                                                                </span>
                                                                <span className={`badge ${projection.reached == null ? "" : projection.reached ? "ok" : "down"}`}>
                                                                    {projection.reached == null ? t("score.not_set") : projection.reached ? t("score.reached") : t("score.not_reached")}
                                                                </span>
                                                            </div>
                                                        </button>
                                                    );
                                                })}
                                            </div>
                                        ) : null}
                                    </div>
                                );
                            }

                            const projection = item.projection;
                            const meta = projectionMetaByLevel.get(projection.level);
                            const levelLabel = meta?.label ?? formatLevelIdToLabel(projection.level);
                            const targetText = meta?.target;
                            return (
                                <button
                                    key={projection.level}
                                    className="target-card"
                                    onClick={() => setSelectedFutureLevel(projection.level)}
                                    style={{
                                        padding: 12,
                                        border: "1px solid var(--border)", borderRadius: 12,
                                        background: "var(--panel-bg)", width: "100%", textAlign: "left", cursor: "pointer",
                                    }}
                                >
                                    <div className="target-card-main" style={{gridTemplateColumns: "minmax(0, 1fr)"}}>
                                        <div className="target-card-copy">
                                            <div style={{fontWeight: 700}}>
                                                {t("score.future_level_title", {level: levelLabel})}
                                            </div>
                                            <div className="future-projection-metrics">
                                                <div className="future-projection-metric">
                                                    <span className="future-projection-label">{t("score.projected_point")}</span>
                                                    <span>{formatFixed2(projection.point)}</span>
                                                </div>
                                                <div className="future-projection-metric">
                                                    <span className="future-projection-label">{t("score.projected_total_point")}</span>
                                                    <span>{formatFixed2(projection.totalPoint)}</span>
                                                </div>
                                                <div className="future-projection-metric">
                                                    <span className="future-projection-label">{t("score.projected_score")} / {t("score.projected_fan")}</span>
                                                    <span>{formatFixed2(projection.score)} / {formatFixed2(projection.fan)}</span>
                                                </div>
                                            </div>
                                        </div>
                                    </div>
                                    <div className="target-card-actions">
                                        <span className="badge">
                                            {t("score.future_target_label")}: {targetText == null ? t("score.future_target_placeholder") : formatTargetText(targetText)}
                                        </span>
                                        <span className={`badge ${projection.reached == null ? "" : projection.reached ? "ok" : "down"}`}>
                                            {projection.reached == null ? t("score.not_set") : projection.reached ? t("score.reached") : t("score.not_reached")}
                                        </span>
                                    </div>
                                </button>
                            );
                        })}
                        {futureProjections.length === 0 ? (
                            <div className="hint">{t("score.future_projection_empty")}</div>
                        ) : null}
                    </div>
                </section>
            </div>

            <Modal
                open={showTileScores}
                onClose={() => setShowTileScores(false)}
                title={t("score.tile_scores_title")}
                width={980}
            >
                <div className="rows">
                    {tileScoreSections.length === 0 ? (
                        <div className="hint">{t("score.tile_scores_empty")}</div>
                    ) : (
                        <div style={{display: "grid", gap: 16, maxHeight: "60vh", overflowY: "auto"}}>
                            {tileScoreSections.map((section) => (
                                <section
                                    key={section.key}
                                    style={{
                                        display: "grid", gap: 10,
                                        padding: 12,
                                        border: "1px solid var(--border)", borderRadius: 14,
                                        background: "linear-gradient(180deg, color-mix(in srgb, var(--panel-bg) 96%, var(--color-ring) 4%), color-mix(in srgb, var(--panel-bg) 88%, transparent))",
                                    }}
                                >
                                    <div style={{fontWeight: 700}}>{section.title}</div>
                                    <div
                                        style={{
                                            display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(86px, 1fr))", gap: 12,
                                        }}
                                    >
                                        {section.entries.map((entry) => (
                                            (() => {
                                                const imageCode = normalizeTileCode(entry.tile);
                                                const isTianDora = tianDoraTileSet.has(entry.tile);
                                                const doraCount = doraCountByTile.get(entry.tile) ?? 0;
                                                const markerLabel =
                                                    isTianDora && doraCount > 0
                                                        ? (doraCount > 1 ? `魂·ドラ×${doraCount}` : "魂·ドラ")
                                                        : isTianDora
                                                            ? "魂"
                                                            : doraCount > 1
                                                                ? `ドラ×${doraCount}`
                                                                : doraCount === 1
                                                                    ? "ドラ"
                                                                    : "";
                                                return (
                                                    <div
                                                        key={entry.tile}
                                                        style={{
                                                            display: "grid", justifyItems: "center",
                                                        }}
                                                    >
                                                        <div
                                                            style={{
                                                                position: "relative", paddingTop: 8,
                                                            }}
                                                        >
                                                            {imageCode ? (
                                                                <img
                                                                    src={`/assets/mahjong/tempai-svg/${imageCode}.svg`}
                                                                    alt={entry.tile}
                                                                    draggable={false}
                                                                    style={{
                                                                        width: 56,
                                                                        height: 76,
                                                                        display: "block", objectFit: "fill",
                                                                    }}
                                                                />
                                                            ) : (
                                                                <div
                                                                    style={{
                                                                        width: 56,
                                                                        height: 76,
                                                                        display: "grid", placeItems: "center", border: "1px solid var(--border)", borderRadius: 8,
                                                                        background: "color-mix(in srgb, var(--panel-bg) 90%, var(--bg))", fontSize: 12,
                                                                        color: "var(--text-muted)",
                                                                    }}
                                                                >
                                                                    {entry.tile}
                                                                </div>
                                                            )}
                                                            <div
                                                                className="badge"
                                                                style={{
                                                                    position: "absolute", top: -8,
                                                                    left: -8,
                                                                    paddingInline: 8,
                                                                    minHeight: 24,
                                                                    borderRadius: 8,
                                                                    borderColor: "color-mix(in srgb, var(--color-ring) 42%, var(--border))",
                                                                    background: "color-mix(in srgb, var(--color-ring) 18%, var(--panel-bg))",
                                                                    color: "var(--text)",
                                                                    boxShadow: "0 8px 18px color-mix(in srgb, var(--color-ring) 16%, transparent)",
                                                                    fontSize: 12,
                                                                    lineHeight: 1.2,
                                                                    maxWidth: 96,
                                                                    display: "grid", placeItems: "center", textAlign: "center", whiteSpace: "nowrap",
                                                                }}
                                                                title={formatFixed2(entry.score)}
                                                            >
                                                                {formatFixed2(entry.score)}
                                                            </div>
                                                            {markerLabel ? (
                                                                <div
                                                                    className="badge"
                                                                    style={{
                                                                        position: "absolute", right: -8,
                                                                        bottom: -8,
                                                                        paddingInline: 8,
                                                                        minHeight: 24,
                                                                        borderRadius: 999,
                                                                        borderColor: "var(--badge-down-border)",
                                                                        background: "var(--badge-down-bg)",
                                                                        color: "var(--badge-down-fg)",
                                                                        boxShadow: "0 8px 18px color-mix(in srgb, var(--badge-down-fg) 16%, transparent)",
                                                                        fontSize: 12,
                                                                        lineHeight: 1.2,
                                                                        display: "grid", placeItems: "center", textAlign: "center", whiteSpace: "nowrap",
                                                                    }}
                                                                    title={markerLabel}
                                                                >
                                                                    {markerLabel}
                                                                </div>
                                                            ) : null}
                                                        </div>
                                                    </div>
                                                );
                                            })()
                                        ))}
                                    </div>
                                </section>
                            ))}
                        </div>
                    )}
                </div>
            </Modal>

            <Modal
                open={selectedExecExplain != null}
                onClose={() => setSelectedExecExplainIndex(null)}
                title={t("score.exec_explain_title")}
                width={680}
            >
                {selectedExecExplain == null ? null : (
                    <div className="rows">
                        <div className="row">
                            <label>{t("score.exec_explain_final")}</label>
                            <div className="badge ok">{selectedExecExplain.effectApplications}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_base_exec")}</label>
                            <div className="badge">{selectedExecExplain.configuredExecutions}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_extra_exec")}</label>
                            <div className="badge">{selectedExecExplain.configuredExtraExecutions}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_manual")}</label>
                            <div className="badge">
                                {selectedExecExplain.manualExtraTriggers > 0
                                    ? `+${selectedExecExplain.manualExtraTriggers}`
                                    : String(selectedExecExplain.manualExtraTriggers)}
                            </div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_adjusted")}</label>
                            <div className="badge">{selectedExecExplain.adjustedActivationCount}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_pre_win")}</label>
                            <div className="badge">{selectedExecExplain.preWinActivationCount}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_deferred_manual")}</label>
                            <div className="badge">{selectedExecExplain.deferredManualTriggerCount}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_transmission")}</label>
                            <div className="badge">{selectedExecExplain.transmissionTriggerCount}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_copied")}</label>
                            <div className="badge">{selectedExecExplain.copiedActivationCount}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_actual_activation")}</label>
                            <div className="badge">{selectedExecExplain.activations}</div>
                        </div>
                        <div className="row">
                            <label>{t("score.exec_explain_actual_execution")}</label>
                            <div className="badge">{selectedExecExplain.executions}</div>
                        </div>
                        <div className="hint">{t("score.exec_explain_hint")}</div>
                    </div>
                )}
            </Modal>

            <Modal
                open={showRuleOverview}
                onClose={() => setShowRuleOverview(false)}
                title={t("score.rule_overview_title")}
                width={980}
            >
                <div style={{display: "grid", gap: 14, maxHeight: "65vh", overflowY: "auto"}}>
                    <section style={{display: "grid", gap: 8}}>
                        <div style={{fontWeight: 700}}>{t("score.rule_overview_registry")}</div>
                        <div style={{display: "flex", gap: 10, alignItems: "center", flexWrap: "wrap"}}>
                            <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                                <span>{t("score.rule_filter_rarity")}</span>
                                <select
                                    value={ruleOverviewRarityFilter}
                                    onChange={(e) => setRuleOverviewRarityFilter(e.target.value)}
                                    style={{width: 180}}
                                >
                                    {rarityOptions.map((rarity) => (
                                        <option key={rarity} value={rarity}>
                                            {rarity === "all" ? t("score.rule_filter_all") : t(`score.rarity_${rarity.toLowerCase()}`)}
                                        </option>
                                    ))}
                                </select>
                            </label>

                            <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                                <span>{t("score.rule_filter_driver")}</span>
                                <select
                                    value={ruleOverviewDriverFilter}
                                    onChange={(e) => setRuleOverviewDriverFilter(e.target.value as DriverFilter)}
                                    style={{width: 180}}
                                >
                                    <option value="all">{t("score.rule_filter_all")}</option>
                                    <option value="code">{t("score.code_driven")}</option>
                                    <option value="config">{t("score.config_driven")}</option>
                                    <option value="unconfigured">{t("score.unconfigured")}</option>
                                </select>
                            </label>
                        </div>

                        {filteredRegistryAmuletOverview.length === 0 ? (
                            <div className="hint">{t("score.rule_overview_registry_empty")}</div>
                        ) : (
                            <div
                                style={{
                                    display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(140px, 1fr))", gap: 10,
                                }}
                            >
                                {filteredRegistryAmuletOverview.map((entry) => (
                                    <div
                                        key={entry.id}
                                        style={{
                                            display: "grid", gap: 8,
                                            justifyItems: "center", padding: 10,
                                            border: "1px solid var(--border)", borderRadius: 10,
                                            background: "var(--panel-bg)",
                                        }}
                                    >
                                        <AmuletCard
                                            item={{
                                                id: entry.id,
                                                uid: entry.id,
                                                volume: 1,
                                                store: [],
                                                tags: [],
                                            }}
                                            scale={0.38}
                                        />
                                        <span className="badge">{t("score.amulet_id_label")}: {entry.id}</span>
                                        <span className={`badge ${entry.driverType === "code" || entry.driverType === "unconfigured" ? "down" : "ok"}`}>
                                            {entry.driverLabel}
                                        </span>
                                        {entry.driverType === "config" ? (
                                            <button
                                                className="nav-btn"
                                                onClick={() => removeConfigForAmuletId(entry.id)}
                                                disabled={!entry.hasCustomConfig}
                                                title={entry.hasCustomConfig ? t("score.delete_config") : t("score.no_custom_config")}
                                            >
                                                {t("score.delete_config")}
                                            </button>
                                        ) : null}
                                    </div>
                                ))}
                            </div>
                        )}
                    </section>
                </div>
            </Modal>

            <Modal
                open={!!selectedFutureProjection}
                onClose={() => setSelectedFutureLevel(null)}
                title={selectedFutureProjection ? t("score.future_growth_detail_title", {
                    level: projectionMetaByLevel.get(selectedFutureProjection.level)?.label ?? formatLevelIdToLabel(selectedFutureProjection.level),
                }) : t("score.future_growth_detail_title", {level: "-"})}
                width={980}
            >
                {selectedFutureProjection ? (
                    <div style={{display: "grid", gap: 14}}>
                        <div className="rows">
                            <div className="row">
                                <label>{t("score.projected_point")}</label>
                                <div className="badge">{formatFixed2(selectedFutureProjection.point)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.projected_total_point")}</label>
                                <div className="badge">{formatFixed2(selectedFutureProjection.totalPoint)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.projected_score")}</label>
                                <div className="badge">{formatFixed2(selectedFutureProjection.score)}</div>
                            </div>
                            <div className="row">
                                <label>{t("score.projected_fan")}</label>
                                <div className="badge">{formatFixed2(selectedFutureProjection.fan)}</div>
                            </div>
                        </div>

                        <div
                            style={{
                                display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))", gap: 12,
                                maxHeight: "60vh", overflowY: "auto",
                            }}
                        >
                            {selectedFutureProjection.amulets.map((amulet) => (
                                <div
                                    key={`${amulet.uid}-${amulet.regId}`}
                                    style={{
                                        display: "grid", gap: 10,
                                        justifyItems: "center", alignContent: "start", padding: 12,
                                        border: "1px solid var(--border)", borderRadius: 12,
                                        background: "var(--panel-bg)",
                                    }}
                                >
                                    <AmuletCard item={amulet.item} scale={0.44}/>
                                    <span className="badge">{t("score.amulet_id_label")}: {amulet.regId}</span>
                                    <div style={{display: "grid", gap: 6, width: "100%"}}>
                                        {safeDisplayStoredDataList(amulet.dataRawList).map((entry) => (
                                            <div
                                                key={`${amulet.uid}-${entry.index}`}
                                                style={{
                                                    display: "grid", gap: 2,
                                                    padding: "8px 10px", border: "1px solid var(--border)", borderRadius: 10,
                                                    background: "color-mix(in srgb, var(--panel-bg) 88%, var(--bg))",
                                                }}
                                            >
                                                <div style={{fontSize: 12, color: "var(--text-muted)"}}>
                                                    {t("score.future_growth_data_raw", {index: entry.index, value: entry.rawDisplay})}
                                                </div>
                                                <div style={{fontWeight: 700}}>
                                                    {t("score.future_growth_data_value", {index: entry.index, value: entry.display})}
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                ) : null}
            </Modal>

            <Modal
                open={!!editingItem}
                onClose={() => setEditingKey(null)}
                title={t("score.amulet_editor_title")}
                width={860}
                actions={(
                    <>
                        <button className="nav-btn" onClick={resetEditor}>
                            {t("score.reset_custom")}
                        </button>
                        <button className="nav-btn" onClick={saveEditor}>
                            {t("score.save_custom")}
                        </button>
                    </>
                )}
            >
                {editingItem ? (
                    <div className="rows">
                        <div className="row">
                            <label>{t("score.amulet_id_label")}</label>
                            <div style={{display: "inline-flex", gap: 8, alignItems: "center", flexWrap: "wrap"}}>
                                <span className="badge">{editingRegId ?? "-"}</span>
                                {isEditingCodeDriven ? (
                                    <span className="badge down">{t("score.code_driven")}</span>
                                ) : null}
                            </div>
                        </div>

                        <div className="notice" style={{marginTop: 0}}>
                            {t("score.formula_vars")}
                            <div className="hint" style={{marginTop: 8}}>
                                {t("score.formula_vars_hint")}
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.current_saved_data_raw")}</label>
                            <div>
                                <div style={{display: "grid", gap: 8}}>
                                    {editingStoredDataList.map((entry) => (
                                        <div key={entry.index} style={{display: "grid", gap: 4}}>
                                            <input
                                                className="form-input"
                                                value={entry.raw}
                                                readOnly
                                            />
                                            <div className="hint">
                                                {t("score.current_saved_data_display_indexed", {
                                                    index: entry.index,
                                                    value: entry.display,
                                                })}
                                            </div>
                                        </div>
                                    ))}
                                </div>
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.current_saved_data_locked")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.exec_count_label")}</label>
                            <input
                                className="form-input"
                                type="number"
                                min={1}
                                max={99}
                                value={draftRule.executions}
                                disabled={isEditingCodeDriven}
                                onChange={(e) => setDraftRule((prev) => ({
                                    ...prev,
                                    executions: clampExecutionCount(e.target.value),
                                }))}
                            />
                        </div>

                        <div className="row">
                            <label>{t("score.extra_exec_count_label")}</label>
                            <input
                                className="form-input"
                                type="number"
                                min={0}
                                max={99}
                                value={draftRule.extraExecutions}
                                disabled={isEditingCodeDriven}
                                onChange={(e) => setDraftRule((prev) => ({
                                    ...prev,
                                    extraExecutions: clampExtraExecutionCount(e.target.value),
                                }))}
                            />
                        </div>

                        <div className="row">
                            <label>{t("score.manual_extra_triggers_label")}</label>
                            <div>
                                <input
                                    className="form-input"
                                    type="text"
                                    inputMode="text"
                                    value={manualExtraTriggerInput}
                                    onChange={(e) => setManualExtraTriggerInput(e.target.value)}
                                />
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.manual_extra_triggers_hint")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.active_on_win_label")}</label>
                            <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                                <input
                                    type="checkbox"
                                    checked={draftRule.activeOnWin}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({
                                        ...prev,
                                        activeOnWin: e.target.checked,
                                    }))}
                                />
                                <span>{draftRule.activeOnWin ? t("score.active_on_win_yes") : t("score.active_on_win_no")}</span>
                            </label>
                        </div>

                        <div className="row">
                            <label>{t("score.growth_after_round_label")}</label>
                            <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                                <input
                                    type="checkbox"
                                    checked={draftRule.growthAfterRound}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({
                                        ...prev,
                                        growthAfterRound: e.target.checked,
                                    }))}
                                />
                                <span>{draftRule.growthAfterRound ? t("score.growth_after_round_yes") : t("score.growth_after_round_no")}</span>
                            </label>
                        </div>

                        <div className="row">
                            <label>{t("score.effect_target_label")}</label>
                            <select
                                value={draftRule.effectTarget}
                                disabled={isEditingCodeDriven}
                                onChange={(e) => setDraftRule((prev) => ({
                                    ...prev,
                                    effectTarget: e.target.value as EffectTarget,
                                }))}
                            >
                                <EffectTargetOptions t={t}/>
                            </select>
                        </div>

                        <div className="row">
                            <label>{t("score.effect_formula_label")}</label>
                            <div>
                                <textarea
                                    className="form-input"
                                    rows={4}
                                    value={draftRule.effectFormula}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({...prev, effectFormula: e.target.value}))}
                                    placeholder={t("score.effect_formula_placeholder")}
                                />
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.effect_formula_hint")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.growth_formula_label")}</label>
                            <div>
                                <textarea
                                    className="form-input"
                                    rows={4}
                                    value={draftRule.growthFormula}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({...prev, growthFormula: e.target.value}))}
                                    placeholder={t("score.growth_formula_placeholder")}
                                />
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.growth_formula_hint")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.win_growth_formula_label")}</label>
                            <div>
                                <textarea
                                    className="form-input"
                                    rows={3}
                                    value={draftRule.winGrowthFormula}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({...prev, winGrowthFormula: e.target.value}))}
                                    placeholder={t("score.win_growth_formula_placeholder")}
                                />
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.win_growth_formula_hint")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.trigger_growth_formula_label")}</label>
                            <div>
                                <textarea
                                    className="form-input"
                                    rows={3}
                                    value={draftRule.triggerGrowthFormula}
                                    disabled={isEditingCodeDriven}
                                    onChange={(e) => setDraftRule((prev) => ({...prev, triggerGrowthFormula: e.target.value}))}
                                    placeholder={t("score.trigger_growth_formula_placeholder")}
                                />
                                <div className="hint" style={{marginTop: 6}}>
                                    {t("score.trigger_growth_formula_hint")}
                                </div>
                            </div>
                        </div>

                        <div className="row">
                            <label>{t("score.note_label")}</label>
                            <textarea
                                className="form-input"
                                rows={3}
                                value={draftRule.note ?? ""}
                                disabled={isEditingCodeDriven}
                                onChange={(e) => setDraftRule((prev) => ({...prev, note: e.target.value}))}
                                placeholder={t("score.note_placeholder")}
                            />
                        </div>
                    </div>
                ) : null}
            </Modal>
        </div>
    );
}
