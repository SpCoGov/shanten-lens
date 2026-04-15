const BASE_UNITS: Array<[number, string]> = [
    [4, "万"],
    [8, "亿"],
    [12, "兆"],
    [16, "京"],
    [20, "垓"],
    [24, "秭"],
    [28, "穰"],
    [32, "沟"],
    [36, "涧"],
    [40, "正"],
    [44, "载"],
    [48, "极"],
];

const HUMAN_UNITS: Array<[number, string]> = [];
for (let repeat = 0; repeat <= 6; repeat += 1) {
    const suffix = "极".repeat(repeat);
    for (const [exponent, label] of BASE_UNITS) {
        const totalExponent = exponent + repeat * 48;
        if (totalExponent < 8 || totalExponent > 300) continue;
        HUMAN_UNITS.push([totalExponent, `${label}${suffix}`]);
    }
}
HUMAN_UNITS.sort((a, b) => a[0] - b[0]);

export function normalizeNumericString(value: string | number | bigint): string {
    const text = String(value ?? "").trim();
    if (!text) return "0";
    if (/^[+-]?\d+$/.test(text)) {
        return BigInt(text).toString();
    }

    const match = text.match(/^([+-]?)(\d+)(?:\.(\d+))?[eE]([+-]?\d+)$/);
    if (match) {
        const sign = match[1] === "-" ? "-" : "";
        const intPart = match[2];
        const fracPart = match[3] ?? "";
        const exponent = Number.parseInt(match[4], 10);
        const digits = `${intPart}${fracPart}`.replace(/^0+/, "") || "0";
        const scale = exponent - fracPart.length;

        if (digits === "0") return "0";
        if (scale >= 0) return `${sign}${digits}${"0".repeat(scale)}`;

        const cutoff = digits.length + scale;
        if (cutoff <= 0) return "0";
        return `${sign}${digits.slice(0, cutoff)}`;
    }

    throw new Error(`invalid numeric literal: ${text}`);
}

export function compareNumericStrings(a: string | number | bigint, b: string | number | bigint): number {
    const left = BigInt(normalizeNumericString(a));
    const right = BigInt(normalizeNumericString(b));
    if (left < right) return -1;
    if (left > right) return 1;
    return 0;
}

export function addNumericStrings(a: string | number | bigint, b: string | number | bigint): string {
    return (BigInt(normalizeNumericString(a)) + BigInt(normalizeNumericString(b))).toString();
}

export function multiplyNumericStrings(a: string | number | bigint, b: string | number | bigint): string {
    return (BigInt(normalizeNumericString(a)) * BigInt(normalizeNumericString(b))).toString();
}

export function divideNumericStrings(
    a: string | number | bigint,
    b: string | number | bigint,
    options?: {
        decimals?: number;
        trimTrailingZeros?: boolean;
    },
): string {
    const decimals = Math.max(0, options?.decimals ?? 5);
    const trimTrailingZeros = options?.trimTrailingZeros ?? true;
    const dividend = BigInt(normalizeNumericString(a));
    const divisor = BigInt(normalizeNumericString(b));
    if (divisor === 0n) {
        throw new Error("Cannot divide by zero");
    }

    const negative = (dividend < 0n) !== (divisor < 0n);
    const left = dividend < 0n ? -dividend : dividend;
    const right = divisor < 0n ? -divisor : divisor;
    const scale = 10n ** BigInt(decimals);
    let quotient = (left * scale) / right;
    const remainder = (left * scale) % right;
    if (remainder * 2n >= right) {
        quotient += 1n;
    }

    let text = quotient.toString();
    if (decimals > 0) {
        if (text.length <= decimals) {
            text = text.padStart(decimals + 1, "0");
        }
        text = `${text.slice(0, -decimals)}.${text.slice(-decimals)}`;
        if (trimTrailingZeros) {
            text = text.replace(/\.?0+$/, "");
        }
    }

    if (negative && text !== "0") {
        text = `-${text}`;
    }
    return text;
}

function formatFixed(value: bigint, exponent: number, decimals: number): string {
    const scale = 10n ** BigInt(exponent);
    let quotient = (value * (10n ** BigInt(decimals))) / scale;
    const remainder = (value * (10n ** BigInt(decimals))) % scale;
    if (remainder * 2n >= scale) {
        quotient += 1n;
    }

    if (decimals <= 0) return quotient.toString();

    let text = quotient.toString();
    if (text.length <= decimals) {
        text = text.padStart(decimals + 1, "0");
    }
    return `${text.slice(0, -decimals)}.${text.slice(-decimals)}`;
}

export function formatLargeNumber(
    value: string | number | bigint,
    options?: {
        humanDecimals?: number;
        scientificDecimals?: number;
    },
): string {
    const humanDecimals = options?.humanDecimals ?? 2;
    const scientificDecimals = options?.scientificDecimals ?? 5;
    const normalized = normalizeNumericString(value);
    const negative = normalized.startsWith("-");
    const digits = negative ? normalized.slice(1) : normalized;
    if (digits === "0") return "0";
    if (digits.length - 1 < 8) return normalized;

    const exponent = digits.length - 1;
    const sign = negative ? "-" : "";
    const number = BigInt(digits);

    if (exponent >= 304) {
        const mantissa = formatFixed(number, exponent, scientificDecimals);
        return `${sign}${mantissa}e${exponent}`;
    }

    let chosen = HUMAN_UNITS[0];
    for (const unit of HUMAN_UNITS) {
        if (unit[0] > exponent) break;
        chosen = unit;
    }
    const [unitExponent, unitLabel] = chosen;
    return `${sign}${formatFixed(number, unitExponent, humanDecimals)}${unitLabel}`;
}
