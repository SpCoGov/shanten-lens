import type {ActionReport} from "../bindings";
import type {GoodsItem} from "./gamestate";
import {compareNumericStrings} from "./bigNumber";

export type BuyPackMode = "single" | "multiple";
export type BuyPackStrategy = "low_to_high" | "high_to_low" | "legendary_only";

export function activeBuyPackHotkeys(settings: {buyPackMode: BuyPackMode; buyPack: string[]}): string[] {
    return settings.buyPackMode === "single" ? settings.buyPack.slice(0, 1) : settings.buyPack;
}

export function selectAmuletPack(
    goods: GoodsItem[],
    coin: string,
    strategy: BuyPackStrategy,
    report?: ActionReport | null,
): GoodsItem | undefined {
    let selected: GoodsItem | undefined;
    for (const good of goods) {
        // Both normal and alternate pack IDs use the last digit as their rarity tier.
        if (![101, 102, 103, 111, 112, 113].includes(good.goodsId) || good.sold) continue;
        const rarity = good.goodsId % 10;
        if (strategy === "legendary_only" && rarity !== 3) continue;
        if (report && report.status !== "unsupported_source") {
            if (report.status !== "ready" || report.stage !== 9 || !report.actions.some((action) =>
                action.kind === "buy_pack" && action.availability === "available"
                && action.params && typeof action.params === "object" && !Array.isArray(action.params)
                && action.params.id === good.id)) continue;
        } else {
            try {
                if (compareNumericStrings(good.price, 0) < 0 || compareNumericStrings(coin, good.price) < 0) continue;
            } catch {
                continue;
            }
        }
        if (!selected || (strategy === "high_to_low" ? rarity > selected.goodsId % 10 : rarity < selected.goodsId % 10)) {
            selected = good;
        }
    }
    return selected;
}
