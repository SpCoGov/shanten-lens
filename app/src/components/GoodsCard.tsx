import React from "react";
import { type GoodsItem } from "../lib/gamestate";

const GOODS_IMG: Record<number, string> = {
    101: "/assets/fx_qingyun_kabao_4.png",
    102: "/assets/fx_qingyun_kabao_3.png",
    103: "/assets/fx_qingyun_kabao_2.png",
};

export default function GoodsCard({
                                      item,
                                      scale = 0.9, // 默认稍大一些，原图 150x220
                                  }: {
    item: GoodsItem;
    scale?: number;
}) {
    const W0 = 150;
    const H0 = 220;
    const W = Math.round(W0 * scale);
    const H = Math.round(H0 * scale);

    const img = GOODS_IMG[item.goodsId] ?? GOODS_IMG[101];

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
                border: "1px solid var(--border, #ddd)",
                background: "#fff",
                boxShadow: "0 4px 12px rgba(0,0,0,.06)",
            }}
            title={`ID=${item.id} goodsId=${item.goodsId} 价格=${item.price}`}
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
                    left: 8,
                    bottom: 8,
                    padding: "4px 8px",
                    borderRadius: 999,
                    background: "rgba(255,255,255,.9)",
                    border: "1px solid var(--border, #ddd)",
                    fontSize: Math.max(12, Math.round(13 * scale)),
                    fontWeight: 700,
                    display: "inline-flex",
                    alignItems: "center",
                    gap: 6,
                    boxShadow: "0 2px 8px rgba(0,0,0,.08)",
                }}
            >
                <span style={{ filter: "drop-shadow(0 1px 1px rgba(0,0,0,.12))" }}>⭐</span>
                <span>{item.price}</span>
            </div>

            {item.sold && (
                <div
                    style={{
                        position: "absolute",
                        inset: 0,
                        background: "rgba(46, 204, 113, 0.32)", // 绿色半透明
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "center",
                        pointerEvents: "none",
                    }}
                >
                    <div
                        style={{
                            padding: "6px 12px",
                            borderRadius: 8,
                            background: "rgba(46, 204, 113, .9)",
                            color: "#fff",
                            fontWeight: 800,
                            letterSpacing: "0.1em",
                            border: "1px solid rgba(0,0,0,.08)",
                            boxShadow: "0 2px 10px rgba(0,0,0,.15)",
                            fontSize: Math.max(12, Math.round(14 * scale)),
                        }}
                    >
                        売却済
                    </div>
                </div>
            )}
        </div>
    );
}