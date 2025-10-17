import React from "react";
import { useFuse, toggleSelect } from "../lib/fuseStore";
import { getRegistry } from "../lib/registryStore";

export default function FuseBar() {
    const { config, selected } = useFuse();
    const reg = getRegistry();

    const aList = config.guard_skip_contains.amulets.map((id) => ({
        kind: "amulet" as const,
        id,
        name: reg.amuletById.get(id)?.name ?? `护符#${id}`,
        icon: `assets/amulet/fu_${String(reg.amuletById.get(id)?.icon_id ?? 0).toString().padStart(4,"0")}.png`,
        chosen: selected.amulets.has(id),
    }));

    const bList = config.guard_skip_contains.badges.map((id) => ({
        kind: "badge" as const,
        id,
        name: reg.badgeById.get(id)?.name ?? `印章#${id}`,
        icon: `assets/badge/badge_${id}.png`,
        chosen: selected.badges.has(id),
    }));

    const items = [
        ...aList.map((x) => ({ ...x, label: "护符" })),
        ...bList.map((x) => ({ ...x, label: "印章" })),
    ];

    if (items.length === 0) {
        return (
            <div style={{ padding: 8, border: "1px dashed #ddd", borderRadius: 10, fontSize: 12, color: "#888" }}>
                空空如也，点击下方按钮添加护身符/印章…
            </div>
        );
    }

    return (
        <div
            style={{
                display: "flex",
                flexWrap: "nowrap",
                overflowX: "auto",
                gap: 8,
                padding: "4px 2px",
            }}
        >
            {items.map((it) => (
                <button
                    key={`${it.kind}-${it.id}`}
                    onClick={() => toggleSelect(it.kind, it.id)}
                    title={`${it.label} ${it.name}（ID:${it.id}）— 点击${it.chosen ? "取消选择" : "选择"}`}
                    style={{
                        display: "flex",
                        alignItems: "center",
                        gap: 8,
                        padding: "6px 10px",
                        borderRadius: 999,
                        border: "1px solid " + (it.chosen ? "#1677ff" : "#ddd"),  // 宽度固定 1px
                        boxShadow: it.chosen ? "0 0 0 2px rgba(22,119,255,.35)" : "none", // 选中外环
                        background: "#fff",
                        cursor: "pointer",
                        whiteSpace: "nowrap",
                        transition: "box-shadow 120ms ease, border-color 120ms ease",
                    }}
                >
                    <img
                        src={it.icon}
                        alt={it.name}
                        onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = "none"; }}
                        style={{ width: 20, height: 20, borderRadius: 4 }}
                        draggable={false}
                    />
                    <span style={{ fontSize: 12, color: "#555" }}>{it.label}</span>
                    <span style={{ fontSize: 12, fontWeight: 600 }}>{it.name}</span>
                    <span style={{ fontSize: 11, color: "#999" }}>#{it.id}</span>
                </button>
            ))}
        </div>
    );
}