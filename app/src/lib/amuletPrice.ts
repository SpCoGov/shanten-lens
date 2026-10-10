import type {EffectItem} from "./gamestate";
import {getRegistry} from "./registryStore";

const RARITY_BASE_PRICE: Record<string, number> = {
    GREEN: 3,
    BLUE: 6,
    ORANGE: 9,
    PURPLE: 12,
    GRAY: 0,
};

export function calcAmuletPrice(item: Pick<EffectItem, "id" | "badge">): number {
    const rawId = Number(item?.id ?? 0);
    if (!Number.isFinite(rawId) || rawId <= 0) return 0;

    const regId = Math.floor(rawId / 10);
    if (regId === 228) return 0;

    const amulet = getRegistry().amuletById.get(regId);
    const catalogPrice = rawId % 10 === 1 ? amulet?.plus_sell_price : amulet?.sell_price;
    const basePrice = catalogPrice ?? RARITY_BASE_PRICE[amulet?.rarity ?? ""] ?? 0;
    const badgeId = Number(item?.badge?.id ?? 0);
    const multiplier = badgeId === 600050 ? 3 : 1;
    return basePrice * multiplier;
}
