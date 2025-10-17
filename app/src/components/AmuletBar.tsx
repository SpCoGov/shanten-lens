import React from "react";
import { type EffectItem } from "../lib/gamestate";
import AmuletCard from "./AmuletCard";

export default function AmuletBar({
                                      items,
                                      scale = 0.55,     // 默认再小一点
                                      max = 8,
                                  }: {
    items: EffectItem[];
    scale?: number;
    max?: number;
}) {
    const list = Array.isArray(items) ? items.slice(0, max) : [];

    if (list.length === 0) {
        return (
            <div
                style={{
                    border: "1px dashed var(--border, #ddd)",
                    borderRadius: 10,
                    padding: 6,
                    color: "var(--muted-fg, #888)",
                    fontSize: 12,
                }}
            >
                暂无护身符
            </div>
        );
    }

    return (
        <div
            style={{
                display: "flex",
                flexWrap: "nowrap",    // 关键：不换行
                gap: 8,
                overflowX: "auto",     // 超出则横向滚动
                overflowY: "hidden",
                padding: "6px 4px",
            }}
        >
            {list.map((it) => (
                <AmuletCard key={`${it.uid}-${it.id}`} item={it} scale={scale} />
            ))}
        </div>
    );
}