import type {ActionCandidate, ActionScore} from "../bindings";

export function actionScoreTotal(score: ActionScore | undefined): number {
    return score?.total ?? 0;
}

export function compareActionScores(a: ActionCandidate, b: ActionCandidate, scores: ReadonlyMap<string, ActionScore>): number {
    const left = a.availability === "available" ? actionScoreTotal(scores.get(a.id)) : null;
    const right = b.availability === "available" ? actionScoreTotal(scores.get(b.id)) : null;
    if (left == null) return right == null ? 0 : 1;
    if (right == null) return -1;
    return right - left;
}
