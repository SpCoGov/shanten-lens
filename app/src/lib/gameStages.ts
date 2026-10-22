import catalog from "../../../backend/assets/levels.json";

const levels = new Map(catalog.levels.map(level => [level.level, level]));
export const gameEnemies = catalog.enemies;

export function getGameLevel(level: number) {
    return levels.get(level);
}

export function getGameStage(level: number, node?: number) {
    return levels.get(level)?.nodes.find(stage => stage.node === node);
}

export function getGameLevels(currentLevel?: number) {
    const activity = levels.get(currentLevel ?? 0)?.activity_id ?? catalog.levels[catalog.levels.length - 1]?.activity_id;
    return catalog.levels.filter(level => level.activity_id === activity);
}

export function getLevelKey(stage: {level: number; node?: number}): string {
    return stage.node == null ? String(stage.level) : `${stage.level}:${stage.node}`;
}
