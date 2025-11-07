import React, { useCallback, useMemo } from "react";
import "../styles/theme.css";
import { useFuse, toggleSelect } from "../lib/fuseStore";
import { getRegistry } from "../lib/registryStore";

type ItemKind = "amulet" | "badge";

type Item = {
    kind: ItemKind;
    id: number;
    name: string;
    icon: string;
    label: string;
    chosen: boolean;
};

export default function FuseBar() {
    const { config, selected } = useFuse();
    const reg = getRegistry();

    const items: Item[] = useMemo(() => {
        const aList = (config.guard_skip_contains.amulets ?? []).map((id: number) => ({
            kind: "amulet" as const,
            id,
            name: reg.amuletById.get(id)?.name ?? `护符#${id}`,
            icon: `assets/amulet/fu_${String(reg.amuletById.get(id)?.icon_id ?? 0).toString().padStart(4, "0")}.png`,
            chosen: selected.amulets.has(id),
            label: "护符",
        }));

        const bList = (config.guard_skip_contains.badges ?? []).map((id: number) => ({
            kind: "badge" as const,
            id,
            name: reg.badgeById.get(id)?.name ?? `印章#${id}`,
            icon: `assets/badge/badge_${id}.png`,
            chosen: selected.badges.has(id),
            label: "印章",
        }));

        return [...aList, ...bList];
    }, [config.guard_skip_contains, reg, selected.amulets, selected.badges]);

    const onToggle = useCallback((kind: ItemKind, id: number) => {
        toggleSelect(kind, id);
    }, []);

    if (items.length === 0) {
        return (
            <div
                style={{
                    padding: 8,
                    border: "1px dashed var(--border)",
                    borderRadius: 10,
                    fontSize: 12,
                    color: "var(--muted)",
                }}
            >
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
                scrollBehavior: "smooth",
            }}
        >
            {items.map((it) => (
                <Pill
                    key={`${it.kind}-${it.id}`}
                    item={it}
                    onToggle={onToggle}
                />
            ))}
        </div>
    );
}

function Pill({
                  item,
                  onToggle,
              }: {
    item: Item;
    onToggle: (kind: ItemKind, id: number) => void;
}) {
    const { kind, id, name, label, chosen, icon } = item;

    const handleKeyDown = useCallback(
        (e: React.KeyboardEvent<HTMLButtonElement>) => {
            if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onToggle(kind, id);
            }
        },
        [kind, id, onToggle]
    );

    const handleImgError = useCallback((e: React.SyntheticEvent<HTMLImageElement>) => {
        const el = e.currentTarget;
        el.style.display = "none";
    }, []);

    const title = `${label} ${name}（ID:${id}）— 点击${chosen ? "取消选择" : "选择"}`;

    return (
        <button
            type="button"
            title={title}
            aria-label={title}
            aria-pressed={chosen}
            onClick={() => onToggle(kind, id)}
            onKeyDown={handleKeyDown}
            style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                padding: "6px 10px",
                borderRadius: 999,
                border: `1px solid ${chosen ? "var(--ring)" : "var(--border)"}`,
                boxShadow: chosen ? "0 0 0 2px rgba(22,119,255,.35)" : "none",
                background: "var(--panel)",
                cursor: "pointer",
                whiteSpace: "nowrap",
                transition: "box-shadow 120ms ease, border-color 120ms ease, transform 60ms ease",
                outline: "none",
            }}
            onMouseDown={(e) => {
                (e.currentTarget as HTMLButtonElement).style.transform = "translateY(1px)";
            }}
            onMouseUp={(e) => {
                (e.currentTarget as HTMLButtonElement).style.transform = "translateY(0)";
            }}
            onBlur={(e) => {
                (e.currentTarget as HTMLButtonElement).style.transform = "translateY(0)";
            }}
        >
            <img
                src={icon}
                alt=""
                aria-hidden="true"
                onError={handleImgError}
                style={{ width: 20, height: 20, borderRadius: 4, flex: "0 0 auto" }}
                draggable={false}
            />
            <span style={{ fontSize: 12, color: "var(--muted)" }}>{label}</span>
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--text)" }}>{name}</span>
            <span style={{ fontSize: 11, color: "var(--muted)" }}>#{id}</span>
        </button>
    );
}
