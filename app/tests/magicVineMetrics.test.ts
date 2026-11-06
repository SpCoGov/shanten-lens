import {hasObtainedMagicVine, highestTileScore} from "../src/lib/magicVineMetrics";

export function runMagicVineMetricsTests() {
    const assert = (value: unknown, message: string) => {if (!value) throw new Error(message);};
    assert(highestTileScore() === null, "missing scores stay unknown");
    assert(highestTileScore({"1m":"15.25","2p":"20","3s":"bad"}) === 2000n, "highest among all suits, with bad entries ignored");
    assert(highestTileScore({"1m":"3.976400e39","9m":"1e39","1p":"3.976400e39"}) === 397640000000000000000000000000000000000000n, "large and repeated scores use exact arithmetic and are not summed");
    const empty={pack_obtained:0,shop_obtained:0,regeneration_obtained:0,other_obtained:0};
    assert(!hasObtainedMagicVine(null), "no current game must not activate statistics from history");
    assert(!hasObtainedMagicVine(empty,[{id:2290}]), "unrelated builds must not activate vine statistics");
    for (const key of Object.keys(empty)) {
        assert(hasObtainedMagicVine({...empty,[key]:1}), "the first acquisition from every source must activate statistics, even after selling it");
    }
    assert(hasObtainedMagicVine(empty,[{id:3760}]) && hasObtainedMagicVine(null,[{id:3761}]), "an already owned normal or PLUS vine must activate after reconnection");
    return "Magic vine metric and visibility checks passed (10 assertions).";
}
