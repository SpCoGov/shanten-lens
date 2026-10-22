import type {GameStateData, MapNodeItem} from "./gamestate";
import {gameEnemies, getGameLevels, getGameStage} from "./gameStages";
import qyzzStageCatalog from "./qyzzStageCatalog.json";

export type LevelTargetEntry = {label: string; level: number; node?: number; target?: string | string[]};

// Local IPC keeps its own stage IDs, progression and score strings.
const QYZZ_LEVEL_TARGETS: LevelTargetEntry[] = qyzzStageCatalog.levels.map(stage => ({
    label: stage.name, level: stage.id, target: stage.target_points,
}));

export function getOrderedLevelTargets(source?: GameStateData["source"], currentLevel?: number, mapNodes: MapNodeItem[] = []): LevelTargetEntry[] {
    if (source === "qyzz") return QYZZ_LEVEL_TARGETS;
    return getGameLevels(currentLevel).flatMap(level => level.nodes.flatMap(stage => {
        if (stage.hp_rate == null) return [];
        const selectedEnemy = level.level === currentLevel ? mapNodes[stage.node - 1]?.subType || undefined : undefined;
        const enemies = gameEnemies.filter(enemy => enemy.group === stage.enemy_group && (selectedEnemy == null || enemy.id === selectedEnemy));
        const targets: Record<string, string | undefined> | null = stage.target_points;
        const possibleTargets = [...new Set(enemies.map(enemy => targets?.[enemy.hp]).filter((target): target is string => target != null))];
        const target = possibleTargets.length === 1 ? possibleTargets[0] : possibleTargets.length > 0 ? possibleTargets : undefined;
        return [{level: level.level, node: stage.node, label: stage.label, target}];
    }));
}

export function getFutureLevelTargets(source: GameStateData["source"], level: number, node?: number, mapNodes: MapNodeItem[] = []): LevelTargetEntry[] {
    const targets = getOrderedLevelTargets(source, level, mapNodes);
    if (source === "qyzz") {
        const index = targets.findIndex(stage => stage.level === level);
        return index < 0 ? [] : targets.slice(index + 1);
    }
    const maps = getGameLevels(level), current = maps.findIndex(map => map.level === level);
    if (current < 0 || !getGameStage(level, node)) return [];
    const order = new Map(maps.map((map, index) => [map.level, index]));
    return targets.filter(stage => order.get(stage.level)! > current || (stage.level === level && stage.node! > node!));
}
