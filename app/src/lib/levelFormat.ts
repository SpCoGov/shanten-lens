export function formatLevelIdToLabel(level: unknown): string {
    const value = Number(level ?? 0);
    if (!Number.isFinite(value) || value <= 0) return "-";

    const text = String(Math.trunc(value));
    const exMatch = text.match(/^12(.+)$/);
    if (exMatch) return `Ex${exMatch[1]}`;

    const normalMatch = text.match(/^1100(.+)$/);
    if (normalMatch) return normalMatch[1]!;

    const chapter = Math.trunc(value / 100);
    const stage = value % 100;
    if (chapter > 0 && stage > 0) return `${chapter}-${stage}`;
    return text;
}

export function parseLevelLabelToId(label: string): number | null {
    const normal = label.match(/^(\d+)-(\d+)$/);
    if (normal) return Number(`${normal[1]}0${normal[2]}`);

    const ex = label.match(/^Ex(\d+)$/i);
    if (ex) return Number(`12${ex[1]}`);

    return null;
}
