import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {createRequire} from "node:module";
import {test} from "node:test";

const require = createRequire(import.meta.url);
const {build} = createRequire(require.resolve("vite"))("esbuild");
const [format, targets, engine] = await Promise.all(
    ["levelFormat", "levelTargets", "scoreEngine"].map(async (name) => {
        const output = await build({
            entryPoints: [require.resolve(`../src/lib/${name}.ts`)],
            bundle: true, write: false, platform: "node", format: "cjs",
        });
        const module = {exports: {}};
        new Function("module", "exports", "require", output.outputFiles[0].text)(module, module.exports, require);
        return module.exports;
    }),
);
const catalog = JSON.parse(readFileSync(new URL("../src/lib/qyzzStageCatalog.json", import.meta.url), "utf8"));

test("local IPC labels use local IDs without changing packet labels or parsing", () => {
    for (const [id, label] of [[101, "1-1"], [503, "5-3"], [1033, "Ex33"], [1199, "Ex199"]]) {
        assert.equal(format.formatLevelIdToLabel(id, "qyzz"), label);
    }
    for (const [id, label] of [[101, "1-1"], [503, "5-3"], [1033, "10-33"], [1199, "11-99"], [1233, "Ex33"], [11001, "1"]]) {
        for (const source of [undefined, null, "packet"]) {
            assert.equal(format.formatLevelIdToLabel(id, source), label);
        }
    }
    assert.equal(format.parseLevelLabelToId("Ex33"), 1233);
    for (const invalid of [undefined, null, 0, -1, NaN, Infinity, "invalid"]) {
        assert.equal(format.formatLevelIdToLabel(invalid, "qyzz"), "-");
    }
});

test("the local catalog preserves all names, exact targets and next-level order", () => {
    const levels = targets.getOrderedLevelTargets("qyzz");
    assert.equal(levels.length, 214);
    assert.equal(new Set(levels.map((item) => item.level)).size, levels.length);
    for (const [index, stage] of catalog.levels.entries()) {
        assert.deepEqual(levels[index], {level: stage.id, label: stage.name, target: stage.target_points});
        assert.equal(format.formatLevelIdToLabel(stage.id, "qyzz"), stage.name);
        assert.equal(levels[index + 1]?.level ?? 0, stage.next_id);
        assert.ok(engine.parseTargetPointValue(stage.target_points) > 0n);
    }
    assert.equal(engine.parseTargetPointValue(levels.find((item) => item.level === 1033).target), 2792n * 10n ** 58n);
    assert.equal(engine.parseTargetPointValue(levels.at(-1).target), 9999n * 10n ** 5154n);
});

test("projections reach Ex199 and selecting a different source restores its own table", () => {
    const packetLevels = targets.getOrderedLevelTargets("packet");
    for (const source of ["qyzz", "packet", "qyzz", undefined, null]) {
        const levels = targets.getOrderedLevelTargets(source);
        if (source !== "qyzz") {
            assert.strictEqual(levels, packetLevels);
            assert.strictEqual(levels, targets.ORDERED_LEVEL_TARGETS);
            assert.equal(levels.find((item) => item.level === 1233).label, "Ex33");
            assert.equal(levels.find((item) => item.level === 1233).target, targets.LEVEL_TARGETS_BY_ID[1233]);
            assert.equal(levels.find((item) => item.level === 1033), undefined);
            continue;
        }
        assert.notStrictEqual(levels, packetLevels);
        for (const [current, next, count] of [[503, 1001, 199], [1033, 1034, 166], [1199, undefined, 0]]) {
            const result = engine.calculateCurrentPoint(100n, 100n, current, []);
            const projections = engine.projectFuturePoints(
                current, levels.slice(levels.findIndex((item) => item.level === current) + 1),
                result, [], 100n, 100n, 1,
            );
            assert.equal(projections.length, count);
            assert.equal(projections[0]?.level, next);
            if (count > 0) assert.equal(projections.at(-1).level, 1199);
            assert.ok(projections.every((item) => item.target > 0n && item.reached === false));
        }
    }
});
