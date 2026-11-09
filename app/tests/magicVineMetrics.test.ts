import {hasObtainedMagicVine, highestTileScore, vineMetricTrend} from "../src/lib/magicVineMetrics";

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
    const current={started_at:1,pack_obtained:208,shop_obtained:7,regeneration_obtained:0,other_obtained:1,farming_obtained:214,farming_rounds:49,shops:7};
    const history=[{...current,started_at:2,pack_obtained:239,other_obtained:0,farming_obtained:244,farming_rounds:55,shops:4},
        {...current,started_at:3,pack_obtained:99,shop_obtained:19,other_obtained:0,farming_obtained:118,farming_rounds:24,shops:11}];
    for (const key of ["obtained","average","shops","purchases"] as const) {
        assert(vineMetricTrend(current,history,key)==="down","each summary metric compares its own historical maximum");
        assert(vineMetricTrend(current,[{...current,started_at:4}],key)==="up","ties stay green");
        assert(vineMetricTrend(current,[{...history[0],started_at:1}],key)==="up","a resumed current game is not its own historical opponent");
    }
    assert(vineMetricTrend({...current,farming_rounds:0},history,"average")==="unknown","an unknown average must not turn green");
    assert(vineMetricTrend(current,[{...current,started_at:5,farming_rounds:0}],"average")==="up","history without confirmed farming rounds has no average");
    assert(vineMetricTrend({...current,farming_obtained:4370,farming_rounds:1000},[{...current,started_at:6,farming_obtained:4371,farming_rounds:1000}],"average")==="down","compare exact ratios before rounding to two decimal places");
    assert(vineMetricTrend(null,history,"obtained")==="unknown","missing current data stays neutral");
    return "Magic vine metric, historical comparison and visibility checks passed.";
}
