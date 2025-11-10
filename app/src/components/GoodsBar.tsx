import React from "react";
import "../styles/theme.css";
import { type GoodsItem } from "../lib/gamestate";
import GoodsCard from "./GoodsCard";
import {t} from "i18next";

export default function GoodsBar({
                                     items,
                                     scale = 0.9,
                                     max = 5,
                                 }: {
    items: GoodsItem[];
    scale?: number;
    max?: number;
}) {
    const list = Array.isArray(items) ? items.slice(0, max) : [];

    if (list.length === 0) {
        return (
            <div
                style={{
                    border: "1px dashed var(--border",
                    borderRadius: 10,
                    padding: 6,
                    color: "var(--muted-fg",
                    fontSize: 12,
                }}
            >
                {t("goods_bar.empty")}
            </div>
        );
    }

    return (
        <div
            style={{
                display: "flex",
                flexWrap: "nowrap",
                gap: 8,
                overflowX: "auto",
                overflowY: "hidden",
                padding: "6px 4px",
            }}
        >
            {list.map((it) => (
                <GoodsCard key={`${it.id}-${it.goodsId}`} item={it} scale={scale} />
            ))}
        </div>
    );
}