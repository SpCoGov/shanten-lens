import React from "react";
import "../styles/theme.css";
import Modal from "./Modal";
import {useRegistry} from "../lib/registryStore";
import {useTranslation} from "react-i18next";

const RARITIES = ["ALL", "PURPLE", "ORANGE", "BLUE", "GREEN"] as const;
type RarityKey = typeof RARITIES[number];

export default function AmuletPickerModal({
                                              open,
                                              onClose,
                                              onSelect,
                                          }: {
    open: boolean;
    onClose: () => void;
    onSelect: (amuletId: number) => void;
}) {
    const {t} = useTranslation();
    const {amulets} = useRegistry();
    const [q, setQ] = React.useState("");
    const [rarity, setRarity] = React.useState<RarityKey>("ALL");
    const [sel, setSel] = React.useState<number | null>(null);

    React.useEffect(() => {
        if (!open) {
            setQ("");
            setRarity("ALL");
            setSel(null);
        }
    }, [open]);

    const list = amulets.filter((a) => {
        const okR = rarity === "ALL" ? true : a.rarity === rarity;
        const key = q.trim().toLowerCase();
        const okQ = key ? a.name.toLowerCase().includes(key) || String(a.id).includes(key) : true;
        return okR && okQ;
    });

    const actions = (
        <button
            className="nav-btn"
            onClick={() => {
                if (sel != null) onSelect(sel);
                onClose();
            }}
            disabled={sel == null}
            title={sel == null ? t("amulet_picker.action_confirm_hint") : undefined}
        >
            {t("amulet_picker.action_confirm")}
        </button>
    );

    return (
        <Modal open={open} onClose={onClose} title={t("amulet_picker.title")} actions={actions}>
            <div style={{display: "flex", gap: 8, marginBottom: 12}}>
                <input
                    value={q}
                    onChange={(e) => setQ(e.target.value)}
                    placeholder={t("amulet_picker.search_ph")}
                    aria-label={t("amulet_picker.search_aria")}
                    style={{
                        flex: 1,
                        padding: "8px 10px",
                        borderRadius: 8,
                        border: "1px solid var(--border)",
                        background: "var(--input-bg)",
                        outline: "none",
                    }}
                    onFocus={(e) => (e.currentTarget.style.boxShadow = "var(--input-focus-ring)")}
                    onBlur={(e) => (e.currentTarget.style.boxShadow = "none")}
                />
                <select
                    value={rarity}
                    onChange={(e) => setRarity(e.target.value as RarityKey)}
                    aria-label={t("amulet_picker.rarity_label")}
                    style={{
                        padding: "8px 10px",
                        borderRadius: 8,
                        border: "1px solid var(--border)",
                        background: "var(--input-bg)",
                        outline: "none",
                    }}
                    onFocus={(e) => (e.currentTarget.style.boxShadow = "var(--input-focus_ring)")}
                    onBlur={(e) => (e.currentTarget.style.boxShadow = "none")}
                >
                    {RARITIES.map((r) => (
                        <option key={r} value={r}>
                            {t(`amulet_picker.rarity.${r}`)}
                        </option>
                    ))}
                </select>
            </div>

            <div
                style={{
                    display: "grid",
                    gridTemplateColumns: "repeat(auto-fill, minmax(140px, 1fr))",
                    gap: 10,
                }}
            >
                {list.map((a) => {
                    const icon = `assets/amulet/fu_${String(a.icon_id).padStart(4, "0")}.png`;
                    const chosen = sel === a.id;
                    return (
                        <button
                            key={a.id}
                            onClick={() => setSel(a.id)}
                            title={t("amulet_picker.card_title", {name: a.name, id: a.id})}
                            style={{
                                textAlign: "left",
                                border: `1px solid ${
                                    chosen
                                        ? "color-mix(in srgb, var(--color-ring) 65%, transparent)"
                                        : "var(--border)"
                                }`,
                                boxShadow: chosen
                                    ? "0 0 0 2px color-mix(in srgb, var(--color-ring) 35%, transparent)"
                                    : "none",
                                borderRadius: 12,
                                padding: 10,
                                background: "var(--panel)",
                                cursor: "pointer",
                                transition: "box-shadow 120ms ease, border-color 120ms ease",
                            }}
                            onMouseEnter={(e) => {
                                if (!chosen) e.currentTarget.style.borderColor = "var(--input-hover-border)";
                            }}
                            onMouseLeave={(e) => {
                                if (!chosen) e.currentTarget.style.borderColor = "var(--border)";
                            }}
                        >
                            <div style={{fontSize: 12, color: "var(--muted)"}}>
                                {t("amulet_picker.field_id", {id: a.id})}
                            </div>
                            <div style={{fontWeight: 600, margin: "4px 0", color: "var(--text)"}}>{a.name}</div>
                            <div
                                style={{
                                    fontSize: 12,
                                    color: "color-mix(in srgb, var(--text) 60%, transparent)",
                                    marginBottom: 6,
                                }}
                            >
                                {t(`amulet_picker.rarity.${a.rarity as RarityKey}`)}
                            </div>
                            <img
                                src={icon}
                                alt={a.name}
                                style={{width: 80, height: "auto", display: "block", opacity: 0.95}}
                                draggable={false}
                            />
                        </button>
                    );
                })}
            </div>
        </Modal>
    );
}