import "../styles/theme.css";
import {type GoodsItem} from "../lib/gamestate";
import {t} from "i18next";
import {formatLargeNumber} from "../lib/bigNumber";

const GOODS_IMG: Record<number, string> = {
    101: "/assets/fx_qingyun_kabao_4.png",
    102: "/assets/fx_qingyun_kabao_3.png",
    103: "/assets/fx_qingyun_kabao_2.png",
};

export default function GoodsCard({
                                      item,
                                      scale = 0.9,
                                      hotkeyLabel,
                                  }: {
    item: GoodsItem;
    scale?: number;
    hotkeyLabel?: string;
}) {
    const W0 = 150;
    const H0 = 220;
    const W = Math.round(W0 * scale);
    const H = Math.round(H0 * scale);
    const compact = scale < 0.55;

    const img = GOODS_IMG[item.goodsId] ?? GOODS_IMG[101];
    const title = t("good_card.tooltip", {
        id: item.id,
        goodsId: item.goodsId,
        price: formatLargeNumber(item.price),
    });
    return (
        <div
            style={{
                position: "relative",
                width: W,
                height: H,
                userSelect: "none",
                flex: "0 0 auto",
                borderRadius: Math.max(8, Math.round(10 * scale)),
                overflow: "hidden",
                border: "1px solid var(--border)",
                background: "var(--panel)",
                boxShadow: "var(--panel-shadow)",
            }}
            title={title}
        >
            <img
                src={img}
                alt=""
                style={{
                    position: "absolute",
                    inset: 0,
                    width: "100%",
                    height: "100%",
                    objectFit: "cover",
                    display: "block",
                }}
                draggable={false}
            />

            <div
                style={{
                    position: "absolute",
                    left: compact ? 3 : 8,
                    bottom: compact ? 3 : 8,
                    padding: compact ? "2px 4px" : "4px 8px",
                    borderRadius: 999,
                    background: "var(--glass-bg)",
                    border: "1px solid var(--border)",
                    fontSize: compact ? 11 : Math.max(12, Math.round(13 * scale)),
                    fontWeight: 700,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: compact ? 3 : 6,
                    boxShadow: "var(--shadow-sm)",
                    backdropFilter: "saturate(120%) blur(2px)",
                }}
            >
                <span style={{filter: "drop-shadow(var(--shadow-text))"}}>⭐</span>
                <span>{formatLargeNumber(item.price)}</span>
            </div>

            {item.sold && (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "var(--sold-scrim)",
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        pointerEvents: "none",
                    }}
                >
                    <div
                        style={{
                            padding: compact ? "3px 4px" : "6px 12px",
                            borderRadius: 8,
                            background: "var(--sold-pill-bg)",
                            color: "var(--on-strong)",
                            fontWeight: 800,
                            letterSpacing: compact ? 0 : "0.1em",
                            border: "1px solid var(--sold-pill-border)",
                            boxShadow: "var(--shadow-md)",
                            fontSize: compact ? 11 : Math.max(12, Math.round(14 * scale)),
                            whiteSpace: "nowrap",
                        }}
                    >
                        {t("good_card.sold")}
                    </div>
                </div>
            )}

            {hotkeyLabel ? (
                <div
                    className="card-hotkey-badge"
                    style={{
                        right: Math.round(7 * scale),
                        bottom: Math.round(7 * scale),
                        minWidth: Math.max(20, Math.round(30 * scale)),
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
