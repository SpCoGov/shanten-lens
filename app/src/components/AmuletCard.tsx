import React from "react";
import "../styles/theme.css";
import {type EffectItem} from "../lib/gamestate";
import {getRegistry} from "../lib/registryStore";
import {t} from "i18next";

const RAR_BG_INDEX: Record<string, number> = {
    PURPLE: 1,
    ORANGE: 2,
    BLUE: 3,
    GREEN: 4,
};

function pad4(n: number) {
    return n.toString().padStart(4, "0");
}

export default function AmuletCard({
                                       item,
                                       scale = 0.65,
                                       onClick,
                                       hotkeyLabel,
                                   }: {
    item: EffectItem;
    scale?: number;
    onClick?: (item: EffectItem) => void;
    hotkeyLabel?: string;
}) {
    const reg = getRegistry();
    const currentTheme = typeof document !== "undefined" ? document.documentElement.getAttribute("data-theme") : null;

    const rawId = item.id;
    const plus = rawId % 10 === 1;
    const regId = Math.floor(rawId / 10);

    const amu = reg.amuletById.get(regId) || null;
    const unknown = !amu;
    const bad = amu && item.badge ? reg.badgeById.get(item.badge.id) || null : null;

    let bgIndex = 4;
    if (currentTheme === "dark-purple") {
        bgIndex = 1;
    } else if (amu && amu.rarity && RAR_BG_INDEX[amu.rarity]) {
        bgIndex = RAR_BG_INDEX[amu.rarity];
    }

    const isWide = item.volume === 2;
    const bgPath = isWide ? `/assets/amulet/fu_widen_bg${bgIndex}.jpg` : `/assets/amulet/fu_bg${bgIndex}.jpg`;
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
    const titleText = unknown
        ? t("amulet_card.title_unknown", { regId, rawId })
        : t("amulet_card.title_known", {
            name: amu!.name,
            suffix: plus ? t("amulet_card.plus_suffix") : ""
        });

    return (
        <div
            style={{
                position: "relative",
                width: W,
                height: H,
                userSelect: "none",
                flex: "0 0 auto",
                cursor: onClick ? "pointer" : "default",
            }}
            title={titleText}
            role={onClick ? "button" : undefined}
            tabIndex={onClick ? 0 : undefined}
            onClick={onClick ? () => onClick(item) : undefined}
            onKeyDown={onClick ? (event) => {
                if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    onClick(item);
                }
            } : undefined}
        >
            {unknown ? (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "color-mix(in srgb, var(--state-down) 75%, transparent)",
                        borderRadius: RADIUS,
                        border: "2px solid color-mix(in srgb, var(--state-down) 55%, transparent)",
                        boxShadow: "var(--shadow-md)",
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
                        border: "1px solid var(--border)",
                        boxShadow: "var(--shadow-sm)",
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
                        filter: "drop-shadow(var(--shadow-text))",
                    }}
                    draggable={false}
                />
            )}

            {badgePath && (
                <img
                    src={badgePath}
                    alt={bad?.name ?? "Badge"}
                    style={{
                        position: "absolute",
                        right: Math.round(4 * scale),
                        top: Math.round(4 * scale),
                        width: BADGE,
                        height: BADGE,
                        filter: "drop-shadow(var(--shadow-text))",
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
                        color: "var(--on-strong)",
                        fontWeight: 700,
                        fontSize: Math.max(12, Math.round(14 * scale)),
                        textShadow: "var(--shadow-text)",
                    }}
                >
                    {t("amulet_card.unknown_label", {regId: regId})}
                </div>
            )}

            {hotkeyLabel ? (
                <div
                    className="card-hotkey-badge"
                    style={{
                        right: Math.round(6 * scale),
                        bottom: Math.round(6 * scale),
                        minWidth: Math.max(20, Math.round(28 * scale)),
                        height: Math.max(18, Math.round(24 * scale)),
                        fontSize: Math.max(11, Math.round(14 * scale)),
                    }}
                >
                    {hotkeyLabel}
                </div>
            ) : null}
        </div>
    );
}
