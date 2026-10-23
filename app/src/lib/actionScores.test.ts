import assert from "node:assert/strict";
import type {ActionCandidate, ActionScore, Availability} from "../bindings";
import {actionScoreTotal, compareActionScores} from "./actionScores";

const action = (id: string, availability: Availability = "available"): ActionCandidate => ({
    id, kind: "skip", params: {}, cost: null, cost_resource: "coin", availability, reasons: [],
});
const totals: [string, number | null][] = [
    ["high", 50], ["fraction", 7.5], ["equal", 7.5], ["zero", 0], ["negative", -10], ["pending", null], ["blocked", 100],
];
const scores = new Map<string, ActionScore>(totals.map(([action_id, total]) => [action_id, {action_id, total, rules: []}]));
const actions = [action("negative"), action("blocked", "unavailable"), action("fraction"), action("pending"),
    action("default"), action("high"), action("equal"), action("zero"), action("unknown", "unknown")];
const before = actions.map(action => action.id);
const sorted = [...actions].sort((a, b) => compareActionScores(a, b, scores));
assert.deepEqual(sorted.map(action => action.id), ["high", "fraction", "equal", "pending", "default", "zero", "negative", "blocked", "unknown"]);
assert.deepEqual(actions.map(action => action.id), before);
assert.equal(actionScoreTotal(undefined), 0);
assert.equal(actionScoreTotal(scores.get("pending")), 0);
assert.equal(actionScoreTotal(scores.get("negative")), -10);
assert.deepEqual([...actions].sort((a, b) => compareActionScores(a, b, new Map())).map(action => action.id),
    ["negative", "fraction", "pending", "default", "high", "equal", "zero", "blocked", "unknown"]);
