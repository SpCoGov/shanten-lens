import assert from "node:assert/strict";
import {buildDebugSnapshotFromState, type GameStateData} from "../src/lib/gamestate";

const state: GameStateData = {
    source: "qyzz", stage: 4, coin: "0", deck_map: {1: "0p", 2: "4p"},
    hand_tiles: [1], last_drawn_tile: 1, dora_tiles: [2], tian_dora_tiles: ["5p"],
    replacement_tiles: [], wall_tiles: [], desktop_remain: 0, locked_tiles: [],
    switch_used_tiles: [], boss_buff: [905],
};
const snapshot = JSON.parse(JSON.stringify(buildDebugSnapshotFromState(state)));
assert.equal(snapshot.source, "qyzz");
assert.equal(snapshot.last_drawn_tile, 1);
assert.equal(buildDebugSnapshotFromState({...state, last_drawn_tile: null}).last_drawn_tile, null);
assert.equal("last_drawn_tile" in buildDebugSnapshotFromState({...state, last_drawn_tile: undefined}), false);
assert.deepEqual(snapshot.boss_buff, [905]);
assert.deepEqual(snapshot.dora_tiles, [2]);
assert.deepEqual(snapshot.tian_dora_tiles, ["5p"]);
assert.equal(snapshot.deck_map[1], "0p");
assert.equal(buildDebugSnapshotFromState({...state, source: "packet"}).source, "packet");
assert.equal(buildDebugSnapshotFromState({...state, source: undefined}).source, null);
console.log("Game-state debug snapshot assertions passed");
