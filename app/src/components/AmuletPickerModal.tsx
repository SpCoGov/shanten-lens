import React from "react";
import Modal from "./Modal";
import {useRegistry} from "../lib/registryStore";

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
        const okQ = q.trim()
            ? a.name.toLowerCase().includes(q.trim().toLowerCase()) || String(a.id).includes(q.trim())
            : true;
        return okR && okQ;
    });

    // 右上角 actions（确认按钮）
    const actions = (
        <button
            className="nav-btn"
            onClick={() => {
                if (sel != null) onSelect(sel);
                onClose();
            }}
            disabled={sel == null}
        >
            确认
        </button>
    );

    return (
        <Modal open={open} onClose={onClose} title="选择护身符" actions={actions}>
            <div style={{display: "flex", gap: 8, marginBottom: 12}}>
                <input
                    value={q}
                    onChange={(e) => setQ(e.target.value)}
                    placeholder="搜索名称或ID…"
                    style={{flex: 1, padding: "8px 10px", borderRadius: 8, border: "1px solid #ddd"}}
                />
                <select
                    value={rarity}
                    onChange={(e) => setRarity(e.target.value as RarityKey)}
                    style={{padding: "8px 10px", borderRadius: 8, border: "1px solid #ddd"}}
                >
                    {RARITIES.map((r) => (
                        <option key={r} value={r}>{r}</option>
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
                            style={{
                                textAlign: "left",
                                border: "1px solid " + (chosen ? "#1677ff" : "#ddd"),
                                boxShadow: chosen ? "0 0 0 2px rgba(22,119,255,.35)" : "none",
                                borderRadius: 12,
                                padding: 10,
                                background: "#fff",
                                cursor: "pointer",
                                transition: "box-shadow 120ms ease, border-color 120ms ease",
                            }}
                        >
                            <div style={{fontSize: 12, color: "#888"}}>ID: {a.id}</div>
                            <div style={{fontWeight: 600, margin: "4px 0"}}>{a.name}</div>
                            <div style={{fontSize: 12, color: "#555", marginBottom: 6}}>{a.rarity}</div>
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