import React from "react";
import {type EffectItem} from "../lib/gamestate";
import {getRegistry} from "../lib/registryStore";

const RAR_BG_INDEX: Record<string, number> = {
    PURPLE: 1,
    ORANGE: 2,
    BLUE: 3,
    GREEN: 4,
};

function pad4(n: number) {
    return n.toString().padStart(4, "0");
}

export default function AmuletCard({item, scale = 0.65}: { item: EffectItem; scale?: number }) {
    const reg = getRegistry();

    const rawId = item.id;
    const plus = rawId % 10 === 1;
    const regId = Math.floor(rawId / 10);

    const amu = reg.amuletById.get(regId) || null;
    const unknown = !amu;
    const bad = amu && item.badge ? reg.badgeById.get(item.badge.id) || null : null;

    let bgIndex = 4;
    if (amu && amu.rarity && RAR_BG_INDEX[amu.rarity]) bgIndex = RAR_BG_INDEX[amu.rarity];

    const isWide = item.volume === 2;
    const bgPath = isWide
        ? `/assets/amulet/fu_widen_bg${bgIndex}.jpg`
        : `/assets/amulet/fu_bg${bgIndex}.jpg`;
    const iconPath = amu ? `/assets/amulet/fu_${pad4(amu.icon_id)}.png` : null;
    const badgePath = item.badge ? `/assets/badge/badge_${item.badge.id}.png` : null;

    // 原始尺寸
    const W0 = isWide ? 320 : 160;
    const H0 = 220;
    const ICON0 = isWide ? 200 : 180;
    const PLUS0 = 50;
    const BADGE0 = 76;

    // 缩放后尺寸
    const W = Math.round(W0 * scale);
    const H = Math.round(H0 * scale);
    const ICON = Math.round(ICON0 * scale);
    const PLUS = Math.round(PLUS0 * scale);
    const BADGE = Math.round(BADGE0 * scale);
    const RADIUS = Math.max(8, Math.round(12 * scale));

    return (
        <div
            style={{
                position: "relative",
                width: W,
                height: H,
                userSelect: "none",
                flex: "0 0 auto",
            }}
            title={
                unknown
                    ? `未知护身符 ID=${regId} (raw=${rawId})`
                    : `${amu!.name}${plus ? "（Plus）" : ""}`
            }
        >
            {unknown ? (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "#ff4d4f",
                        borderRadius: RADIUS,
                        border: "2px solid #b71c1c",
                    }}
                />
            ) : (
                <img
                    src={bgPath}
                    alt=""
                    style={{
                        position: "absolute",
                        inset: 0,
                        width: "100%",
                        height: "100%",
                        objectFit: "cover",
                        borderRadius: RADIUS,
                        border: "1px solid var(--border, #ddd)",
                    }}
                    draggable={false}
                />
            )}

            {!unknown && iconPath && (
                <img
                    src={iconPath}
                    alt={amu!.name}
                    style={{
                        position: "absolute",
                        left: "50%",
                        top: "50%",
                        transform: "translate(-50%, -50%)",
                        width: ICON,
                        height: "auto",
                        pointerEvents: "none",
                    }}
                    draggable={false}
                />
            )}

            {plus && (
                <img
                    src="/assets/amulet/plus.png"
                    alt="Plus"
                    style={{
                        position: "absolute",
                        left: Math.round(6 * scale),
                        bottom: Math.round(6 * scale),
                        width: PLUS,
                        height: PLUS,
                        filter: "drop-shadow(0 1px 2px rgba(0,0,0,.35))",
                    }}
                    draggable={false}
                />
            )}

            {badgePath && (
                <img
                    src={badgePath}
                    alt="Badge"
                    style={{
                        position: "absolute",
                        right: Math.round(4 * scale),
                        top: Math.round(4 * scale),
                        width: BADGE,
                        height: BADGE,
                        filter: "drop-shadow(0 1px 2px rgba(0,0,0,.35))",
                    }}
                    draggable={false}
                />
            )}

            {unknown && (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        color: "#fff",
                        fontWeight: 700,
                        fontSize: Math.max(12, Math.round(14 * scale)),
                        textShadow: "0 1px 2px rgba(0,0,0,.35)",
                    }}
                >
                    {`未知ID ${regId}`}
                </div>
            )}
        </div>
    );
}