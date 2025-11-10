import React from "react";
import "../styles/theme.css";
import Modal from "./Modal";
import { useRegistry } from "../lib/registryStore";
import { useTranslation } from "react-i18next";

const RARS = ["ALL", "RED", "BLUE", "BROWN"] as const;
type Rar = typeof RARS[number];

export default function BadgePickerModal({
                                             open,
                                             onClose,
                                             onSelect,
                                         }: {
    open: boolean;
    onClose: () => void;
    onSelect: (badgeId: number) => void;
}) {
    const { t } = useTranslation();
    const { badges } = useRegistry();
    const [q, setQ] = React.useState("");
    const [rar, setRar] = React.useState<Rar>("ALL");
    const [sel, setSel] = React.useState<number | null>(null);

    React.useEffect(() => {
        if (!open) {
            setQ("");
            setRar("ALL");
            setSel(null);
        }
    }, [open]);

    const list = badges.filter((b) => {
        const okR = rar === "ALL" ? true : b.rarity === rar;
        const key = q.trim().toLowerCase();
        const okQ = key ? b.name.toLowerCase().includes(key) || String(b.id).includes(key) : true;
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
            title={sel == null ? t("badge_picker.action_confirm_hint") : undefined}
        >
            {t("badge_picker.action_confirm")}
        </button>
    );

    return (
        <Modal open={open} onClose={onClose} title={t("badge_picker.title")} actions={actions}>
            <div style={{ display: "flex", gap: 8, marginBottom: 12 }}>
                <input
                    value={q}
                    onChange={(e) => setQ(e.target.value)}
                    placeholder={t("badge_picker.search_ph")}
                    aria-label={t("badge_picker.search_aria")}
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
                    value={rar}
                    onChange={(e) => setRar(e.target.value as Rar)}
                    aria-label={t("badge_picker.rarity_label")}
                    style={{
                        padding: "8px 10px",
                        borderRadius: 8,
                        border: "1px solid var(--border)",
                        background: "var(--input-bg)",
                        outline: "none",
                    }}
                    onFocus={(e) => (e.currentTarget.style.boxShadow = "var(--input-focus-ring)")}
                    onBlur={(e) => (e.currentTarget.style.boxShadow = "none")}
                >
                    {RARS.map((r) => (
                        <option key={r} value={r}>
                            {t(`badge_picker.rarity.${r}`)}
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
                {list.map((b) => {
                    const icon = `assets/badge/badge_${b.id}.png`;
                    const chosen = sel === b.id;
                    return (
                        <button
                            key={b.id}
                            onClick={() => setSel(b.id)}
                            title={t("badge_picker.card_title", { name: b.name, id: b.id })}
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
                            <div style={{ fontSize: 12, color: "var(--muted)" }}>
                                {t("badge_picker.field_id", { id: b.id })}
                            </div>
                            <div style={{ fontWeight: 600, margin: "4px 0", color: "var(--text)" }}>{b.name}</div>
                            <div
                                style={{
                                    fontSize: 12,
                                    color: "color-mix(in srgb, var(--text) 60%, transparent)",
                                    marginBottom: 6,
                                }}
                            >
                                {t(`badge_picker.rarity.${b.rarity as Rar}`)}
                            </div>
                            <img
                                src={icon}
                                alt={b.name}
                                style={{ width: 72, height: 72, display: "block", opacity: 0.95 }}
                                draggable={false}
                            />
                        </button>
                    );
                })}
            </div>
        </Modal>
    );
}