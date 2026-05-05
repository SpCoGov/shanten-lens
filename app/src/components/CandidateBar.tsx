import React from "react";
import "../styles/theme.css";
import AmuletCard from "./AmuletCard";
import { type CandidateEffectRef, type EffectItem } from "../lib/gamestate";
import {t} from "i18next";

function toEffectItem(c: CandidateEffectRef): EffectItem {
    return {
        id: c.id,
        uid: 0,
        volume: 1,
        store: [],
        tags: [],
        badge: c.badgeId
            ? { id: c.badgeId, uid: 0, random: 0, store: [] }
            : undefined,
    };
}

export default function CandidateBar({
                                         candidates,
                                         scale = 0.55,
                                         max = 8,
                                         onCandidateClick,
                                         hotkeyLabels,
                                     }: {
    candidates: CandidateEffectRef[];
    scale?: number;
    max?: number;
    onCandidateClick?: (candidate: CandidateEffectRef) => void;
    hotkeyLabels?: string[];
}) {
    const list = Array.isArray(candidates) ? candidates.slice(0, max) : [];

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
                {t("candidate_amulet_empty")}
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
            {list.map((c, index) => {
                const eff: EffectItem = toEffectItem(c);
                return (
                    <AmuletCard
                        key={`cand-${c.id}-${c.badgeId}`}
                        item={eff}
                        scale={scale}
                        onClick={onCandidateClick ? () => onCandidateClick(c) : undefined}
                        hotkeyLabel={hotkeyLabels?.[index]}
                    />
                );
            })}
        </div>
    );
}
