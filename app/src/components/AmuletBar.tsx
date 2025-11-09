import React from "react";
import "../styles/theme.css";
import {type EffectItem} from "../lib/gamestate";
import AmuletCard from "./AmuletCard";
import "../lib/i18n"
import {useTranslation} from "react-i18next";

export default function AmuletBar({
                                      items,
                                      scale = 0.55,
                                      max = 8,
                                  }: {
    items: EffectItem[];
    scale?: number;
    max?: number;
}) {
    const list = Array.isArray(items) ? items.slice(0, max) : [];
    const {t} = useTranslation();
    if (list.length === 0) {
        return (
            <div
                style={{
                    border: "1px dashed var(--border)",
                    borderRadius: 10,
                    padding: 6,
                    color: "var(--muted-fg)",
                    fontSize: 12,
                }}
            >
                {t("noAmulet")}
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
                <AmuletCard key={`${it.uid}-${it.id}`} item={it} scale={scale}/>
            ))}
        </div>
    );
}