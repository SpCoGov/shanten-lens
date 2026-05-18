import {
    BASE_UNIT_EXPONENTS,
    formatLargeNumber,
    formatLargeScaledNumber,
    getLargeNumberHumanUnits,
    normalizeNumericString,
} from "./bigNumber";
import type {EffectItem} from "./gamestate";
import {
    getRegisteredAmuletRule,
    getRegisteredAmuletRuleExact,
    registerAmuletRule,
    unregisterAmuletRule,
} from "./amuletRuleRegistry";

export const SCALE_DECIMALS = 2;
export const SCALE = 100n;

export type EffectTarget = "none" | "score" | "fan";

export type FormulaVars = {
    data: bigint;
    dataValues: bigint[];
    score: bigint;
    fan: bigint;
    level: bigint;
    execution: bigint;
    extra_execution: bigint;
    activation: bigint;
};

export type AmuletRuleConfig = {
    dataRaw: string;
    executions: number;
    extraExecutions: number;
    manualExtraTriggers: number;
    activeOnWin: boolean;
    growthAfterRound: boolean;
    forceTransmissionSeal: boolean;
    disableFutureGrowth: boolean;
    effectTarget: EffectTarget;
    effectFormula: string;
    growthFormula: string;
    triggerGrowthFormula: string;
    winGrowthFormula: string;
    note?: string;
};

export type ResolvedAmuletRule = AmuletRuleConfig & {
    item: EffectItem;
    regId: number;
    dataRawList: string[];
    badgeId: number | null;
    hasExtensionSeal: boolean;
    hasTransmissionSeal: boolean;
    hasAngelSeal: boolean;
};

export type CurrentPointResult = {
    baseScore: bigint;
    finalScore: bigint;
    baseFan: bigint;
    finalFan: bigint;
    finalPoint: bigint;
    perAmulet: Array<{
        uid: number;
        regId: number;
        executions: number;
        extraExecutions: number;
        manualExtraTriggers: number;
        activations: number;
        effectApplications: number;
        configuredExecutions: number;
        configuredExtraExecutions: number;
        adjustedActivationCount: number;
        preWinActivationCount: number;
        deferredManualTriggerCount: number;
        transmissionTriggerCount: number;
        copiedActivationCount: number;
        dataRaw: string;
        scoreAfter: bigint;
        fanAfter: bigint;
        pointAfter: bigint;
    }>;
};

export type FutureProjection = {
    level: number;
    point: bigint;
    totalPoint: bigint;
    score: bigint;
    fan: bigint;
    target?: bigint | null;
    reached: boolean | null;
    amulets: Array<{
        uid: number;
        regId: number;
        item: EffectItem;
        dataRaw: string;
        dataRawList: string[];
    }>;
};

export type AmuletRuleRuntimeState = {
    score: bigint;
    fan: bigint;
};

export type AmuletRuntimeContext = {
    hasPinzuInHand?: boolean;
    soulTileCount?: number;
};

export type CalculatePointOptions = {
    freezeFutureGrowth?: boolean;
};

export type AmuletEffectContext = {
    rule: ResolvedAmuletRule;
    index: number;
    rules: ResolvedAmuletRule[];
    level: number;
    executionIndex: number;
    activationIndex: number;
    triggerCount: number;
    activationCount: number;
    extraExecutionCount: number;
    activationCounts: number[];
    executionCounts: number[];
    effectApplicationCounts: number[];
    state: AmuletRuleRuntimeState;
    runtime: AmuletRuntimeContext;
    vars: FormulaVars;
    applyCopiedActivation: (targetIndex: number) => void;
};

export type AmuletActivationContext = {
    rule: ResolvedAmuletRule;
    index: number;
    rules: ResolvedAmuletRule[];
    level: number;
    runtime: AmuletRuntimeContext;
};

export type AmuletGrowthContext = {
    rule: ResolvedAmuletRule;
    index: number;
    rules: ResolvedAmuletRule[];
    level: number;
    executionIndex: number;
    currentResult: CurrentPointResult;
    currentData: bigint;
    vars: FormulaVars;
};

export type AmuletEffectResult = (Partial<Pick<AmuletRuleRuntimeState, "score" | "fan">> & {
    transmissionTriggers?: number;
}) | null | void;

export type AmuletRuleHandler = {
    getDefaultConfig?: (item: EffectItem) => Partial<AmuletRuleConfig> | null | undefined;
    affectsPoint?: boolean;
    getMaxEffectApplications?: (context: AmuletActivationContext) => number | null | undefined;
    getPreWinActivationCount?: (context: AmuletActivationContext) => number | null | undefined;
    isActiveOnWin?: (context: AmuletActivationContext) => boolean;
    applyEffect?: (context: AmuletEffectContext) => AmuletEffectResult;
    applyTriggerGrowth?: (context: AmuletEffectContext) => string[] | null | undefined;
    getGrowthRepeat?: (context: Omit<AmuletGrowthContext, "executionIndex" | "currentData" | "vars">) => number | null | undefined;
    growData?: (context: AmuletGrowthContext) => bigint | null | undefined;
};

const BADGE_EXTENSION_SEAL_ID = 600160;
const BADGE_TRANSMISSION_SEAL_ID = 600170;
const BADGE_ANGEL_SEAL_ID = 600190;

const COMPAT_BASE_UNITS = [
    ["万", "万"],
    ["亿", "億"],
    ["兆", "兆"],
    ["京", "京"],
    ["垓", "垓"],
    ["秭", "秭"],
    ["穰", "穣"],
    ["沟", "溝"],
    ["涧", "澗"],
    ["正", "正"],
    ["载", "載"],
    ["极", "極"],
] as const;

function getUnitExponents(): Map<string, number> {
    const unitExponents = new Map<string, number>(getLargeNumberHumanUnits().map(([exponent, label]) => [label, exponent]));
    for (let repeat = 0; repeat <= 6; repeat += 1) {
        for (const [index, exponent] of BASE_UNIT_EXPONENTS.entries()) {
            for (const label of COMPAT_BASE_UNITS[index]) {
                for (const repeatLabel of ["极", "極"]) {
                    unitExponents.set(`${label}${repeatLabel.repeat(repeat)}`, exponent + repeat * 48);
                }
            }
        }
    }
    return unitExponents;
}

export const DEFAULT_RULE_CONFIG: AmuletRuleConfig = {
    dataRaw: "0",
    executions: 1,
    extraExecutions: 0,
    manualExtraTriggers: 0,
    activeOnWin: false,
    growthAfterRound: false,
    forceTransmissionSeal: false,
    disableFutureGrowth: false,
    effectTarget: "none",
    effectFormula: "",
    growthFormula: "data",
    triggerGrowthFormula: "",
    winGrowthFormula: "",
    note: "",
};

export const PRESET_AMULET_RULES: Record<number, Partial<AmuletRuleConfig>> = {};
export {getRegisteredAmuletRule, getRegisteredAmuletRuleExact, registerAmuletRule, unregisterAmuletRule};

export function clampExecutionCount(value: number | string | null | undefined): number {
    const n = Number(value);
    if (!Number.isFinite(n)) return 1;
    return Math.max(1, Math.min(99, Math.trunc(n)));
}

export function clampExtraExecutionCount(value: number | string | null | undefined): number {
    const n = Number(value);
    if (!Number.isFinite(n)) return 0;
    return Math.max(0, Math.min(99, Math.trunc(n)));
}

export function clampManualExtraTriggerCount(value: number | string | null | undefined): number {
    const n = Number(value);
    if (!Number.isFinite(n)) return 0;
    return Math.max(-99, Math.min(99, Math.trunc(n)));
}

export function parseStoredDataFromItem(item: EffectItem): string {
    return parseStoredDataListFromItem(item)[0] ?? "0";
}

export function parseStoredDataListFromItem(item: EffectItem): string[] {
    if (!Array.isArray(item.store)) return [];
    const values: string[] = [];
    for (const entry of item.store) {
        const text = String(entry ?? "").trim();
        if (/^[+-]?\d+$/.test(text)) values.push(normalizeNumericString(text));
    }
    return values;
}

export function resolveAmuletRule(item: EffectItem, custom?: Partial<AmuletRuleConfig> | null): ResolvedAmuletRule {
    const regId = item.id;
    const badgeId = typeof item.badge?.id === "number" ? item.badge.id : null;
    const registeredRule = getRegisteredAmuletRule(regId);
    const codeDefaultConfig = registeredRule?.getDefaultConfig?.(item) ?? {};
    const preset = PRESET_AMULET_RULES[regId] ?? {};
    const storedDataRawList = parseStoredDataListFromItem(item);
    const fallbackRawData = String(codeDefaultConfig.dataRaw ?? preset.dataRaw ?? "0").trim();
    const dataRawList = storedDataRawList.length > 0
        ? storedDataRawList
        : (/^[+-]?\d+$/.test(fallbackRawData) ? [normalizeNumericString(fallbackRawData)] : ["0"]);
    const rawData = dataRawList[0] ?? "0";
    const merged: AmuletRuleConfig = {
        ...DEFAULT_RULE_CONFIG,
        ...codeDefaultConfig,
        ...preset,
        ...custom,
        dataRaw: /^[+-]?\d+$/.test(rawData) ? normalizeNumericString(rawData) : "0",
        executions: clampExecutionCount(custom?.executions ?? preset.executions ?? codeDefaultConfig.executions ?? 1),
        extraExecutions: clampExtraExecutionCount(custom?.extraExecutions ?? preset.extraExecutions ?? codeDefaultConfig.extraExecutions ?? 0),
        manualExtraTriggers: clampManualExtraTriggerCount(custom?.manualExtraTriggers ?? preset.manualExtraTriggers ?? codeDefaultConfig.manualExtraTriggers ?? 0),
        activeOnWin: custom?.activeOnWin ?? preset.activeOnWin ?? codeDefaultConfig.activeOnWin ?? ((custom?.effectTarget ?? preset.effectTarget ?? codeDefaultConfig.effectTarget ?? "none") !== "none"),
        growthAfterRound: custom?.growthAfterRound ?? preset.growthAfterRound ?? codeDefaultConfig.growthAfterRound ?? false,
        forceTransmissionSeal: custom?.forceTransmissionSeal ?? preset.forceTransmissionSeal ?? codeDefaultConfig.forceTransmissionSeal ?? false,
        disableFutureGrowth: custom?.disableFutureGrowth ?? preset.disableFutureGrowth ?? codeDefaultConfig.disableFutureGrowth ?? false,
        effectTarget: custom?.effectTarget ?? preset.effectTarget ?? codeDefaultConfig.effectTarget ?? "none",
        effectFormula: custom?.effectFormula ?? preset.effectFormula ?? codeDefaultConfig.effectFormula ?? "",
        growthFormula: custom?.growthFormula ?? preset.growthFormula ?? codeDefaultConfig.growthFormula ?? "data",
        triggerGrowthFormula: custom?.triggerGrowthFormula ?? preset.triggerGrowthFormula ?? codeDefaultConfig.triggerGrowthFormula ?? "",
        winGrowthFormula: custom?.winGrowthFormula ?? preset.winGrowthFormula ?? codeDefaultConfig.winGrowthFormula ?? "",
        note: custom?.note ?? preset.note ?? codeDefaultConfig.note ?? "",
    };
    return {
        ...merged,
        item,
        regId,
        dataRawList,
        badgeId,
        hasExtensionSeal: badgeId === BADGE_EXTENSION_SEAL_ID,
        hasTransmissionSeal: badgeId === BADGE_TRANSMISSION_SEAL_ID || merged.forceTransmissionSeal,
        hasAngelSeal: badgeId === BADGE_ANGEL_SEAL_ID,
    };
}

export function parseFixed2(value: string | number | bigint): bigint {
    if (typeof value === "bigint") return value * SCALE;
    const text = String(value ?? "").trim();
    if (!text) return 0n;
    const match = text.match(/^([+-]?)(\d+)(?:\.(\d+))?$/);
    if (!match) {
        return BigInt(normalizeNumericString(text)) * SCALE;
    }
    const sign = match[1] === "-" ? -1n : 1n;
    const whole = BigInt(match[2] || "0");
    const frac = (match[3] ?? "").padEnd(SCALE_DECIMALS, "0").slice(0, SCALE_DECIMALS);
    const fracValue = BigInt(frac || "0");
    return sign * (whole * SCALE + fracValue);
}

export function parseTargetPointValue(value: string | number | bigint): bigint | null {
    const text = String(value ?? "").trim();
    if (!text) return null;

    if (/^[+-]?\d+(?:\.\d+)?$/.test(text)) {
        return parseFixed2(text);
    }

    const sci = text.match(/^([+-]?\d+(?:\.\d+)?)[eE]([+-]?\d+)$/);
    if (sci) {
        const scaled = parseFixed2(sci[1]);
        const exponent = Number.parseInt(sci[2], 10);
        if (!Number.isFinite(exponent)) return null;
        if (exponent >= 0) return scaled * (10n ** BigInt(exponent));
        const divisor = 10n ** BigInt(-exponent);
        return scaled / divisor;
    }

    const unitMatch = text.match(/^([+-]?\d+(?:\.\d+)?)(.+)$/);
    if (!unitMatch) return null;
    const numeric = parseFixed2(unitMatch[1]);
    const unit = unitMatch[2].trim();
    const exponent = getUnitExponents().get(unit);
    if (exponent == null) return null;
    return numeric * (10n ** BigInt(exponent));
}

export function fixed2ToString(value: bigint, trimTrailingZeros = true): string {
    const negative = value < 0n;
    const abs = negative ? -value : value;
    const whole = abs / SCALE;
    const frac = abs % SCALE;
    let text = `${whole.toString()}.${frac.toString().padStart(SCALE_DECIMALS, "0")}`;
    if (trimTrailingZeros) text = text.replace(/\.?0+$/, "");
    if (negative && text !== "0") text = `-${text}`;
    return text;
}

export function formatFixed2(value: bigint): string {
    return formatLargeScaledNumber(value, SCALE_DECIMALS);
}

export function multiplyFixed2(a: bigint, b: bigint): bigint {
    return (a * b) / SCALE;
}

function powFixed2(base: bigint, exponentScaled: bigint): bigint {
    if (exponentScaled < 0n) throw new Error("negative exponent is not supported");
    if (exponentScaled % SCALE !== 0n) throw new Error("exponent must be an integer");
    const exponent = exponentScaled / SCALE;
    let result = SCALE;
    let factor = base;
    let power = exponent;
    while (power > 0n) {
        if (power % 2n === 1n) {
            result = multiplyFixed2(result, factor);
        }
        power /= 2n;
        if (power > 0n) {
            factor = multiplyFixed2(factor, factor);
        }
    }
    return result;
}

class FormulaParser {
    private readonly text: string;
    private index = 0;
    private readonly vars: FormulaVars;

    constructor(text: string, vars: FormulaVars) {
        this.text = text;
        this.vars = vars;
    }

    parse(): bigint {
        const value = this.parseExpression();
        this.skipWhitespace();
        if (this.index < this.text.length) {
            throw new Error(`Unexpected token at ${this.index + 1}`);
        }
        return value;
    }

    private parseExpression(): bigint {
        let value = this.parseTerm();
        while (true) {
            this.skipWhitespace();
            const ch = this.peek();
            if (ch === "+") {
                this.index += 1;
                value += this.parseTerm();
                continue;
            }
            if (ch === "-") {
                this.index += 1;
                value -= this.parseTerm();
                continue;
            }
            return value;
        }
    }

    private parseTerm(): bigint {
        let value = this.parsePower();
        while (true) {
            this.skipWhitespace();
            if (this.peek() !== "*") return value;
            this.index += 1;
            value = multiplyFixed2(value, this.parsePower());
        }
    }

    private parsePower(): bigint {
        let value = this.parseUnary();
        this.skipWhitespace();
        if (this.peek() === "^") {
            this.index += 1;
            value = powFixed2(value, this.parsePower());
        }
        return value;
    }

    private parseUnary(): bigint {
        this.skipWhitespace();
        const ch = this.peek();
        if (ch === "+") {
            this.index += 1;
            return this.parseUnary();
        }
        if (ch === "-") {
            this.index += 1;
            return -this.parseUnary();
        }
        return this.parsePrimary();
    }

    private parsePrimary(): bigint {
        this.skipWhitespace();
        const ch = this.peek();
        if (ch === "(") {
            this.index += 1;
            const value = this.parseExpression();
            this.skipWhitespace();
            if (this.peek() !== ")") throw new Error("Missing closing parenthesis");
            this.index += 1;
            return value;
        }
        if (/[0-9.]/.test(ch ?? "")) {
            return this.parseNumber();
        }
        if (/[a-zA-Z_]/.test(ch ?? "")) {
            return this.parseIdentifier();
        }
        throw new Error(`Unexpected token at ${this.index + 1}`);
    }

    private parseNumber(): bigint {
        const start = this.index;
        while (this.index < this.text.length && /[0-9.]/.test(this.text[this.index])) {
            this.index += 1;
        }
        return parseFixed2(this.text.slice(start, this.index));
    }

    private parseIdentifier(): bigint {
        const start = this.index;
        while (this.index < this.text.length && /[a-zA-Z0-9_]/.test(this.text[this.index])) {
            this.index += 1;
        }
        const name = this.text.slice(start, this.index).toLowerCase();
        if (name === "data") {
            this.skipWhitespace();
            if (this.peek() === "[") {
                this.index += 1;
                this.skipWhitespace();
                const indexStart = this.index;
                while (this.index < this.text.length && /[0-9]/.test(this.text[this.index])) {
                    this.index += 1;
                }
                const rawIndex = this.text.slice(indexStart, this.index);
                this.skipWhitespace();
                if (this.peek() !== "]") throw new Error("Missing closing bracket");
                this.index += 1;
                const itemIndex = Number.parseInt(rawIndex, 10);
                if (!Number.isFinite(itemIndex) || itemIndex < 0) {
                    throw new Error("Invalid data index");
                }
                return this.vars.dataValues[itemIndex] ?? 0n;
            }
        }
        if (!(name in this.vars)) {
            throw new Error(`Unknown variable: ${name}`);
        }
        if (name === "data") {
            return this.vars.data;
        }
        return this.vars[name as Exclude<keyof FormulaVars, "dataValues" | "data">];
    }

    private skipWhitespace() {
        while (this.index < this.text.length && /\s/.test(this.text[this.index])) {
            this.index += 1;
        }
    }

    private peek() {
        return this.text[this.index];
    }
}

export function evaluateFormula(formula: string, vars: FormulaVars): bigint {
    const source = String(formula ?? "").trim();
    if (!source) return vars.data;
    return new FormulaParser(source, vars).parse();
}

export function computeBaseScore(
    handTileIds: number[],
    deckMap: Map<number, string>,
    tileScoreMap?: Record<string, string> | null,
): bigint {
    if (!Array.isArray(handTileIds) || handTileIds.length === 0 || !tileScoreMap) return 0n;
    return handTileIds.reduce((sum, tileId) => {
        const face = deckMap.get(tileId);
        if (!face) return sum;
        return sum + parseFixed2(tileScoreMap[face] ?? "0");
    }, 0n);
}

function computePoint(score: bigint, fan: bigint): bigint {
    return multiplyFixed2(score, fan);
}

function getTriggerActivationCount(rule: ResolvedAmuletRule): number {
    const extensionMultiplier = rule.hasExtensionSeal ? 2 : 1;
    return clampExecutionCount(rule.executions) * extensionMultiplier;
}

function getExtraActivationCount(rule: ResolvedAmuletRule): number {
    return clampExtraExecutionCount(rule.extraExecutions);
}

function getEffectiveActivationCount(rule: ResolvedAmuletRule): number {
    return getTriggerActivationCount(rule) + getExtraActivationCount(rule);
}

function applyDefaultRuleEffect(rule: ResolvedAmuletRule, vars: FormulaVars, state: AmuletRuleRuntimeState) {
    if (rule.effectTarget === "score" && rule.effectFormula.trim()) {
        try {
            state.score = evaluateFormula(rule.effectFormula, vars);
        } catch {
        }
    } else if (rule.effectTarget === "fan" && rule.effectFormula.trim()) {
        try {
            state.fan = evaluateFormula(rule.effectFormula, vars);
        } catch {
        }
    }
}

function applyRegisteredRuleEffect(context: AmuletEffectContext) {
    const handler = getRegisteredAmuletRule(context.rule.regId);
    if (!handler?.applyEffect) {
        applyDefaultRuleEffect(context.rule, context.vars, context.state);
        return {transmissionTriggers: undefined, affectsPoint: context.rule.effectTarget !== "none"};
    }
    const result = handler.applyEffect(context);
    if (result?.score != null) context.state.score = result.score;
    if (result?.fan != null) context.state.fan = result.fan;
    return {
        transmissionTriggers: result?.transmissionTriggers,
        affectsPoint: !!handler.affectsPoint,
    };
}

function applyDefaultGrowth(rule: ResolvedAmuletRule, vars: FormulaVars, currentData: bigint) {
    try {
        const grown = evaluateFormula(rule.growthFormula || "data", vars);
        return applyAngelSealGrowth(rule, currentData, grown);
    } catch {
        return currentData;
    }
}

function applyAngelSealGrowth(rule: ResolvedAmuletRule, currentData: bigint, grown: bigint) {
    return rule.hasAngelSeal ? currentData + (grown - currentData) * 2n : grown;
}

function applyRegisteredGrowth(context: AmuletGrowthContext) {
    const handler = getRegisteredAmuletRule(context.rule.regId);
    if (!handler?.growData) {
        return applyDefaultGrowth(context.rule, context.vars, context.currentData);
    }
    try {
        const result = handler.growData(context);
        if (typeof result === "bigint") {
            return applyAngelSealGrowth(context.rule, context.currentData, result);
        }
    } catch {
    }
    return applyDefaultGrowth(context.rule, context.vars, context.currentData);
}

function getGrowthRepeatCount(
    rule: ResolvedAmuletRule,
    index: number,
    rules: ResolvedAmuletRule[],
    result: CurrentPointResult,
    level: number,
) {
    const handler = getRegisteredAmuletRule(rule.regId);
    if (handler?.getGrowthRepeat) {
        try {
            const repeat = handler.getGrowthRepeat({
                rule,
                index,
                rules,
                level,
                currentResult: result,
            });
            const normalized = Number(repeat);
            if (Number.isFinite(normalized) && normalized >= 0) {
                return Math.max(0, Math.trunc(normalized));
            }
        } catch {
        }
    }
    return rule.hasExtensionSeal ? 2 : 1;
}

function isRuleActiveOnWin(
    rule: ResolvedAmuletRule,
    index: number,
    rules: ResolvedAmuletRule[],
    level: number,
    runtime: AmuletRuntimeContext,
) {
    if (!rule.activeOnWin) return false;
    const handler = getRegisteredAmuletRule(rule.regId);
    if (!handler?.isActiveOnWin) return true;
    try {
        return !!handler.isActiveOnWin({
            rule,
            index,
            rules,
            level,
            runtime,
        });
    } catch {
        return true;
    }
}

function getMaxEffectApplicationCount(
    rule: ResolvedAmuletRule,
    index: number,
    rules: ResolvedAmuletRule[],
    level: number,
    runtime: AmuletRuntimeContext,
) {
    const handler = getRegisteredAmuletRule(rule.regId);
    if (!handler?.getMaxEffectApplications) return null;
    try {
        const limit = handler.getMaxEffectApplications({
            rule,
            index,
            rules,
            level,
            runtime,
        });
        const normalized = Number(limit);
        if (!Number.isFinite(normalized)) return null;
        return Math.max(0, Math.trunc(normalized));
    } catch {
        return null;
    }
}

function getPreWinActivationCount(
    rule: ResolvedAmuletRule,
    index: number,
    rules: ResolvedAmuletRule[],
    level: number,
    runtime: AmuletRuntimeContext,
) {
    const handler = getRegisteredAmuletRule(rule.regId);
    if (!handler?.getPreWinActivationCount) return 0;
    try {
        const count = handler.getPreWinActivationCount({
            rule,
            index,
            rules,
            level,
            runtime,
        });
        const normalized = Number(count);
        if (!Number.isFinite(normalized)) return 0;
        return Math.max(0, Math.trunc(normalized));
    } catch {
        return 0;
    }
}

function applyTriggerGrowthForRule(context: AmuletEffectContext) {
    const handler = getRegisteredAmuletRule(context.rule.regId);
    if (!handler?.applyTriggerGrowth) return;
    try {
        const nextDataRawList = handler.applyTriggerGrowth(context);
        if (!Array.isArray(nextDataRawList) || nextDataRawList.length === 0) return;
        context.rule.dataRawList = nextDataRawList.map((value) => {
            const text = String(value ?? "").trim();
            return /^[+-]?\d+$/.test(text) ? normalizeNumericString(text) : "0";
        });
        context.rule.dataRaw = context.rule.dataRawList[0] ?? "0";
    } catch {
    }
}

function applyFormulaDataGrowth(rule: ResolvedAmuletRule, formula: string, vars: FormulaVars) {
    const source = String(formula ?? "").trim();
    if (!source) return;
    try {
        const assignment = source.match(/^(data(?:\[(\d+)])?)\s*=\s*(.+)$/);
        if (!assignment) return;
        const targetIndex = Number.parseInt(assignment[2] ?? "0", 10);
        const expression = assignment[3] ?? "";
        if (!Number.isFinite(targetIndex) || targetIndex < 0) return;
        const nextData = evaluateFormula(expression, vars);
        const nextDataRawList = [...rule.dataRawList];
        while (nextDataRawList.length <= targetIndex) {
            nextDataRawList.push("0");
        }
        nextDataRawList[targetIndex] = nextData.toString();
        rule.dataRawList = nextDataRawList;
        rule.dataRaw = nextDataRawList[0] ?? "0";
    } catch {
    }
}

export function calculateCurrentPoint(
    baseScore: bigint,
    baseFan: bigint,
    level: number,
    amuletRules: ResolvedAmuletRule[],
    runtime: AmuletRuntimeContext = {},
    options: CalculatePointOptions = {},
): CurrentPointResult {
    const state: AmuletRuleRuntimeState = {score: baseScore, fan: baseFan};
    const activationCounts = amuletRules.map(() => 0);
    const executionCounts = amuletRules.map(() => 0);
    const effectApplicationCounts = amuletRules.map(() => 0);
    const transmissionTriggerCounts = amuletRules.map(() => 0);
    const copiedActivationCounts = amuletRules.map(() => 0);
    const preWinActivationCounts = amuletRules.map((rule, index) =>
        getPreWinActivationCount(rule, index, amuletRules, level, runtime),
    );
    const levelValue = BigInt(level) * SCALE;

    amuletRules.forEach((rule) => {
        const dataValues = rule.dataRawList.map((value) => {
            try {
                return BigInt(value || "0");
            } catch {
                return 0n;
            }
        });
        if (options.freezeFutureGrowth && rule.disableFutureGrowth) {
            return;
        }
        applyFormulaDataGrowth(rule, rule.winGrowthFormula, {
            data: dataValues[0] ?? 0n,
            dataValues,
            score: state.score,
            fan: state.fan,
            level: levelValue,
            execution: SCALE,
            extra_execution: BigInt(getExtraActivationCount(rule)) * SCALE,
            activation: SCALE,
        });
    });

    const applyEffectApplication = (
        index: number,
        triggerCount: number,
        activationCount: number,
        extraExecutionCount: number,
        effectSequence: number,
        executionSequence: number,
        allowTransmission: boolean,
    ): { triggered: boolean; transmissionCount: number } => {
        const rule = amuletRules[index];
        if (!rule || !isRuleActiveOnWin(rule, index, amuletRules, level, runtime)) {
            return {triggered: false, transmissionCount: 0};
        }
        const maxEffectApplications = getMaxEffectApplicationCount(rule, index, amuletRules, level, runtime);
        if (maxEffectApplications != null && effectApplicationCounts[index] >= maxEffectApplications) {
            return {triggered: false, transmissionCount: 0};
        }

        const activationIndex = effectSequence;
        const executionIndex = executionSequence;
        const prevScore = state.score;
        const prevFan = state.fan;
        const prevPoint = computePoint(prevScore, prevFan);
        const dataValues = rule.dataRawList.map((value) => {
            try {
                return BigInt(value || "0");
            } catch {
                return 0n;
            }
        });
        const vars: FormulaVars = {
            data: dataValues[0] ?? 0n,
            dataValues,
            score: state.score,
            fan: state.fan,
            level: levelValue,
            execution: BigInt(executionIndex) * SCALE,
            extra_execution: BigInt(extraExecutionCount) * SCALE,
            activation: BigInt(activationIndex) * SCALE,
        };
        const effectContext: AmuletEffectContext = {
            rule,
            index,
            rules: amuletRules,
            level,
            executionIndex,
            activationIndex,
            triggerCount,
            activationCount,
            extraExecutionCount,
            activationCounts,
            executionCounts,
            effectApplicationCounts,
            state,
            runtime,
            vars,
            applyCopiedActivation: (targetIndex: number) => {
                const targetRule = amuletRules[targetIndex];
                if (!targetRule) return;
                const targetTriggerCount = getTriggerActivationCount(targetRule);
                const targetExtraExecutionCount = getExtraActivationCount(targetRule);
                const targetActivationCount = getEffectiveActivationCount(targetRule);
                const copiedAttempt = applyEffectApplication(
                    targetIndex,
                    targetTriggerCount,
                    targetActivationCount,
                    targetExtraExecutionCount,
                    effectApplicationCounts[targetIndex] + 1,
                    Math.max(1, executionCounts[targetIndex] + 1),
                    false,
                );
                if (copiedAttempt.triggered) {
                    copiedActivationCounts[targetIndex] += 1;
                }
            },
        };
        const effectResult = applyRegisteredRuleEffect(effectContext);
        const nextPoint = computePoint(state.score, state.fan);
        const pointRelevant = effectResult.affectsPoint;
        const triggered = !pointRelevant || nextPoint !== prevPoint;

        if (!triggered) {
            state.score = prevScore;
            state.fan = prevFan;
            return {triggered: false, transmissionCount: 0};
        }

        if (!(options.freezeFutureGrowth && rule.disableFutureGrowth)) {
            applyTriggerGrowthForRule(effectContext);
            applyFormulaDataGrowth(rule, rule.triggerGrowthFormula, {
                ...vars,
                score: state.score,
                fan: state.fan,
                data: (() => {
                    try {
                        return BigInt(rule.dataRaw || "0");
                    } catch {
                        return 0n;
                    }
                })(),
                dataValues: rule.dataRawList.map((value) => {
                    try {
                        return BigInt(value || "0");
                    } catch {
                        return 0n;
                    }
                }),
            });
        }
        effectApplicationCounts[index] += 1;
        return {
            triggered: true,
            transmissionCount: allowTransmission ? Math.max(0, effectResult.transmissionTriggers ?? 1) : 0,
        };
    };

    const triggerRule = (index: number, consumedBaseActivations = 0, consumedDeferredActivations = 0) => {
        const rule = amuletRules[index];
        if (!rule || !isRuleActiveOnWin(rule, index, amuletRules, level, runtime)) return;

        const triggerCount = getTriggerActivationCount(rule);
        const extraExecutionCount = getExtraActivationCount(rule);
        const activationCount = getEffectiveActivationCount(rule);
        const adjustedActivationCount = Math.max(0, activationCount + rule.manualExtraTriggers);
        const baseActivationCap = Math.max(0, Math.min(activationCount, adjustedActivationCount) - consumedBaseActivations);
        const deferredManualTriggerCount = Math.max(0, adjustedActivationCount - activationCount - consumedDeferredActivations);

        for (let activation = 0; activation < baseActivationCap; activation += 1) {
            const countsAsExecution = activation < triggerCount;
            const executionSequence = countsAsExecution ? executionCounts[index] + 1 : Math.max(1, executionCounts[index]);
            const activationSequence = activationCounts[index] + 1;
            const effectAttempt = applyEffectApplication(
                index,
                triggerCount,
                adjustedActivationCount,
                extraExecutionCount,
                activationSequence,
                executionSequence,
                countsAsExecution,
            );
            if (!effectAttempt.triggered) {
                continue;
            }
            activationCounts[index] += 1;
            if (countsAsExecution) {
                executionCounts[index] += 1;
            }
            if (countsAsExecution && amuletRules[index + 1]?.hasTransmissionSeal) {
                for (let transmission = 0; transmission < effectAttempt.transmissionCount; transmission += 1) {
                    transmissionTriggerCounts[index + 1] += 1;
                    triggerRule(index + 1);
                }
            }
        }

        for (let activation = 0; activation < deferredManualTriggerCount; activation += 1) {
            const executionSequence = executionCounts[index] + 1;
            const activationSequence = activationCounts[index] + 1;
            const effectAttempt = applyEffectApplication(
                index,
                triggerCount + deferredManualTriggerCount,
                adjustedActivationCount,
                extraExecutionCount,
                activationSequence,
                executionSequence,
                true,
            );
            if (!effectAttempt.triggered) {
                continue;
            }
            activationCounts[index] += 1;
            executionCounts[index] += 1;
            if (amuletRules[index + 1]?.hasTransmissionSeal) {
                for (let transmission = 0; transmission < effectAttempt.transmissionCount; transmission += 1) {
                    transmissionTriggerCounts[index + 1] += 1;
                    triggerRule(index + 1);
                }
            }
        }
    };

    for (let index = 0; index < amuletRules.length; index += 1) {
        const rule = amuletRules[index];
        if (!rule || !isRuleActiveOnWin(rule, index, amuletRules, level, runtime)) continue;
        const triggerCount = getTriggerActivationCount(rule);
        const extraExecutionCount = getExtraActivationCount(rule);
        const activationCount = getEffectiveActivationCount(rule);
        const adjustedActivationCount = Math.max(0, activationCount + rule.manualExtraTriggers);
        const preCount = Math.min(preWinActivationCounts[index] ?? 0, adjustedActivationCount);
        if (preCount <= 0) continue;

        for (let activation = 0; activation < preCount; activation += 1) {
            const executionSequence = executionCounts[index] + 1;
            const activationSequence = activationCounts[index] + 1;
            const effectAttempt = applyEffectApplication(
                index,
                triggerCount,
                adjustedActivationCount,
                extraExecutionCount,
                activationSequence,
                executionSequence,
                true,
            );
            if (!effectAttempt.triggered) {
                continue;
            }
            activationCounts[index] += 1;
            executionCounts[index] += 1;
            if (amuletRules[index + 1]?.hasTransmissionSeal) {
                for (let transmission = 0; transmission < effectAttempt.transmissionCount; transmission += 1) {
                    transmissionTriggerCounts[index + 1] += 1;
                    triggerRule(index + 1);
                }
            }
        }
    }

    for (let index = 0; index < amuletRules.length; index += 1) {
        const triggerCount = getTriggerActivationCount(amuletRules[index]!);
        const extraExecutionCount = getExtraActivationCount(amuletRules[index]!);
        const activationCount = triggerCount + extraExecutionCount;
        const adjustedActivationCount = Math.max(0, activationCount + amuletRules[index]!.manualExtraTriggers);
        const consumedPreCount = Math.min(preWinActivationCounts[index] ?? 0, Math.min(activationCount, adjustedActivationCount));
        triggerRule(index, consumedPreCount, 0);
    }

    const perAmulet: CurrentPointResult["perAmulet"] = amuletRules.map((rule, index) => ({
        uid: rule.item.uid,
        regId: rule.regId,
        executions: executionCounts[index],
        extraExecutions: Math.max(0, activationCounts[index] - executionCounts[index]),
        manualExtraTriggers: rule.manualExtraTriggers,
        activations: activationCounts[index],
        effectApplications: effectApplicationCounts[index],
        configuredExecutions: getTriggerActivationCount(rule),
        configuredExtraExecutions: getExtraActivationCount(rule),
        adjustedActivationCount: Math.max(0, getEffectiveActivationCount(rule) + rule.manualExtraTriggers),
        preWinActivationCount: Math.min(
            preWinActivationCounts[index] ?? 0,
            Math.max(0, getEffectiveActivationCount(rule) + rule.manualExtraTriggers),
        ),
        deferredManualTriggerCount: Math.max(0, Math.max(0, getEffectiveActivationCount(rule) + rule.manualExtraTriggers) - getEffectiveActivationCount(rule)),
        transmissionTriggerCount: transmissionTriggerCounts[index],
        copiedActivationCount: copiedActivationCounts[index],
        dataRaw: rule.dataRaw,
        scoreAfter: state.score,
        fanAfter: state.fan,
        pointAfter: computePoint(state.score, state.fan),
    }));

    return {
        baseScore,
        finalScore: state.score,
        baseFan,
        finalFan: state.fan,
        finalPoint: computePoint(state.score, state.fan),
        perAmulet,
    };
}

function simulateLevelWins(
    baseScore: bigint,
    baseFan: bigint,
    level: number,
    rules: ResolvedAmuletRule[],
    winCount: number,
    runtime: AmuletRuntimeContext,
) {
    const normalizedWinCount = Math.max(1, Math.trunc(winCount));
    let firstResult: CurrentPointResult | null = null;
    let lastResult: CurrentPointResult | null = null;
    for (let winIndex = 0; winIndex < normalizedWinCount; winIndex += 1) {
        const result = calculateCurrentPoint(baseScore, baseFan, level, rules, runtime, {freezeFutureGrowth: true});
        if (firstResult == null) firstResult = result;
        lastResult = result;
    }
    return {
        firstResult: firstResult!,
        lastResult: lastResult!,
    };
}

function growAmuletData(
    rules: ResolvedAmuletRule[],
    result: CurrentPointResult,
    level: number,
    growthAfterRound: boolean | null = null,
): ResolvedAmuletRule[] {
    const levelValue = BigInt(level) * SCALE;
    return rules.map((rule, index) => {
        if (rule.disableFutureGrowth) {
            return rule;
        }
        if (growthAfterRound != null && rule.growthAfterRound !== growthAfterRound) {
            return rule;
        }
        let nextData = BigInt(rule.dataRaw || "0");
        const growthRepeat = getGrowthRepeatCount(rule, index, rules, result, level);

        for (let executionIndex = 1; executionIndex <= growthRepeat; executionIndex += 1) {
            const currentData = nextData;
            const dataValues = rule.dataRawList.map((value, dataIndex) => {
                if (dataIndex === 0) return currentData;
                try {
                    return BigInt(value || "0");
                } catch {
                    return 0n;
                }
            });
            const vars: FormulaVars = {
                data: dataValues[0] ?? currentData,
                dataValues,
                score: result.finalScore,
                fan: result.finalFan,
                level: levelValue,
                execution: BigInt(executionIndex) * SCALE,
                extra_execution: BigInt(getExtraActivationCount(rule)) * SCALE,
                activation: BigInt(executionIndex + getExtraActivationCount(rule)) * SCALE,
            };
            nextData = applyRegisteredGrowth({
                rule,
                index,
                rules,
                level,
                executionIndex,
                currentResult: result,
                currentData,
                vars,
            });
        }

        return {
            ...rule,
            dataRaw: nextData.toString(),
            dataRawList: [nextData.toString(), ...rule.dataRawList.slice(1)],
        };
    });
}

export function projectFuturePoints(
    currentLevel: number,
    futureLevels: Array<{ level: number; target?: string }>,
    currentResult: CurrentPointResult,
    currentRules: ResolvedAmuletRule[],
    baseScore: bigint,
    baseFan: bigint,
    winCount: number,
    runtime: AmuletRuntimeContext = {},
): FutureProjection[] {
    const out: FutureProjection[] = [];
    let level = currentLevel;
    let rules = currentRules;
    let result = currentResult;

    for (const future of futureLevels) {
        level = future.level;
        rules = growAmuletData(rules, result, level, false);
        const simulated = simulateLevelWins(baseScore, baseFan, level, rules, winCount, runtime);
        const projectedSingleWinResult = simulated.firstResult;
        result = simulated.lastResult;
        const target = parseTargetPointValue(future.target ?? "") ?? null;
        const totalPoint = projectedSingleWinResult.finalPoint * BigInt(Math.max(1, winCount));
        out.push({
            level,
            point: projectedSingleWinResult.finalPoint,
            totalPoint,
            score: projectedSingleWinResult.finalScore,
            fan: projectedSingleWinResult.finalFan,
            target,
            reached: target == null ? null : totalPoint >= target,
            amulets: rules.map((rule) => ({
                uid: rule.item.uid,
                regId: rule.regId,
                item: rule.item,
                dataRaw: rule.dataRaw,
                dataRawList: [...rule.dataRawList],
            })),
        });
        rules = growAmuletData(rules, result, level, true);
    }

    return out;
}
