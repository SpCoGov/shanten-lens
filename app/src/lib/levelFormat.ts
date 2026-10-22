import type {GameStateData} from "./gamestate";
import {getGameLevel, getGameLevels, getGameStage} from "./gameStages";
import qyzzStages from "./qyzzStageCatalog.json";

export function formatLevelIdToLabel(level: unknown, source?: GameStateData["source"], node?: number): string {
    const value = Number(level ?? 0);
    if (!Number.isFinite(value) || value <= 0) return "-";
    if (source === "qyzz") {
        const stage = qyzzStages.levels.find(stage => stage.id === value);
        if (stage) return stage.name;
    } else {
        const map = getGameLevel(value);
        if (map) return getGameStage(value, node)?.label ?? map.label;
    }

    const text = String(Math.trunc(value));
    if (source === "qyzz" && value > 1000) return `Ex${Math.trunc(value) - 1000}`;
    const exMatch = text.match(/^12(.+)$/);
    if (exMatch) return `Ex${exMatch[1]}`;

    const normalMatch = text.match(/^1100(.+)$/);
    if (normalMatch) return normalMatch[1]!;

    const chapter = Math.trunc(value / 100);
    const stage = value % 100;
    if (chapter > 0 && stage > 0) return `${chapter}-${stage}`;
    return text;
}

export function parseLevelLabelToId(label: string, source?: GameStateData["source"], currentLevel?: number): number | null {
    if (source === "qyzz") return qyzzStages.levels.find(stage => stage.name === label)?.id ?? null;
    const level = getGameLevels(currentLevel).find(level => level.label === label || level.nodes.some(node => node.label === label));
    if (level) return level.level;
    const normal = label.match(/^(\d+)-(\d+)$/);
    if (normal) return Number(`${normal[1]}0${normal[2]}`);

    const ex = label.match(/^Ex(\d+)$/i);
    if (ex) return Number(`12${ex[1]}`);

    return null;
}
