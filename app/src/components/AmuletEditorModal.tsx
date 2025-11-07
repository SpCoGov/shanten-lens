import React from "react";
import "../styles/theme.css";
import Modal from "./Modal";
import AmuletPickerModal from "./AmuletPickerModal";
import BadgePickerModal from "./BadgePickerModal";
import AmuletCard from "./AmuletCard";
import {useRegistry} from "../lib/registryStore";

export type EditedAmulet = { id: number; plus: boolean; badge: number | null };

export default function AmuletEditorModal({
                                              open,
                                              onClose,
                                              title = "编辑护身符",
                                              initial,
                                              onConfirm,
                                          }: {
    open: boolean;
    onClose: () => void;
    title?: string;
    initial?: Partial<EditedAmulet>;
    onConfirm: (data: EditedAmulet) => void;
}) {
    const {amuletById, badgeById} = useRegistry();

    const [amuletId, setAmuletId] = React.useState<number | null>(initial?.id ?? null);
    const [plus, setPlus] = React.useState<boolean>(Boolean(initial?.plus));
    const [badgeId, setBadgeId] = React.useState<number | null>(
        typeof initial?.badge === "number" ? initial!.badge : null
    );

    const [pickA, setPickA] = React.useState(false);
    const [pickB, setPickB] = React.useState(false);

    React.useEffect(() => {
        if (!open) return;
        setAmuletId(initial?.id ?? null);
        setPlus(Boolean(initial?.plus));
        setBadgeId(typeof initial?.badge === "number" ? initial!.badge : null);
    }, [open, initial?.id, initial?.plus, initial?.badge]);

    const actions = (
        <>
            <button
                className="nav-btn"
                onClick={() =>
                    onConfirm({
                        id: amuletId as number,
                        plus,
                        badge: badgeId ?? null,
                    })
                }
                disabled={amuletId == null}
            >
                确认
            </button>
        </>
    );

    const reg = amuletId != null ? amuletById.get(amuletId) || null : null;
    const rawId = amuletId != null ? amuletId * 10 + (plus ? 1 : 0) : 0;
    const effectItem =
        amuletId != null
            ? ({
                id: rawId,
                volume: 1,
                badge: badgeId != null ? {id: badgeId} : undefined,
            } as any)
            : null;

    const focusRing = (el: HTMLElement | null, on: boolean) => {
        if (!el) return;
        el.style.boxShadow = on ? "var(--input-focus-ring)" : "none";
        el.style.borderColor = on ? "var(--input-hover-border)" : "var(--border)";
    };

    return (
        <>
            <Modal open={open} onClose={onClose} title={title} actions={actions} width={760}>
                <div style={{display: "grid", gap: 12, marginBottom: 12}}>
                    {/* 护身符 ID */}
                    <div style={{display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap"}}>
                        <label style={{width: 84, color: "var(--muted)"}}>护身符ID</label>
                        <input
                            type="number"
                            value={amuletId ?? ""}
                            onChange={(e) => {
                                const v = e.target.value.trim();
                                setAmuletId(v === "" ? null : Number(v));
                            }}
                            placeholder="输入护身符ID…"
                            style={{
                                width: 160,
                                padding: "8px 10px",
                                borderRadius: 8,
                                border: "1px solid var(--border)",
                                background: "var(--input-bg)",
                                outline: "none",
                            }}
                            onFocus={(e) => focusRing(e.currentTarget, true)}
                            onBlur={(e) => focusRing(e.currentTarget, false)}
                        />
                        <button className="nav-btn" onClick={() => setPickA(true)}>
                            从列表选择
                        </button>
                        <span style={{color: "var(--muted)", fontSize: 12}}>
              {amuletId != null ? (reg ? ` ${reg.name}` : " 未知ID") : " 未选择"}
            </span>
                    </div>

                    <div style={{display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap"}}>
                        <label style={{width: 84, color: "var(--muted)"}}>Plus</label>
                        <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                            <input type="checkbox" checked={plus} onChange={(e) => setPlus(e.target.checked)}/>
                            <span>Plus</span>
                        </label>
                    </div>

                    <div style={{display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap"}}>
                        <label style={{width: 84, color: "var(--muted)"}}>印章</label>
                        <input
                            type="number"
                            value={badgeId ?? ""}
                            onChange={(e) => {
                                const v = e.target.value.trim();
                                setBadgeId(v === "" ? null : Number(v));
                            }}
                            placeholder="输入印章ID…"
                            style={{
                                width: 160,
                                padding: "8px 10px",
                                borderRadius: 8,
                                border: "1px solid var(--border)",
                                background: "var(--input-bg)",
                                outline: "none",
                            }}
                            onFocus={(e) => focusRing(e.currentTarget, true)}
                            onBlur={(e) => focusRing(e.currentTarget, false)}
                        />
                        <button className="nav-btn" onClick={() => setPickB(true)}>
                            从列表选择
                        </button>
                        <button className="nav-btn" onClick={() => setBadgeId(null)} disabled={badgeId == null}>
                            清除
                        </button>
                        <span style={{color: "var(--muted)", fontSize: 12}}>
              {badgeId != null ? badgeById.get(badgeId)?.name ?? "未知印章" : "未选择"}
            </span>
                    </div>
                </div>

                <div
                    style={{
                        border: "1px solid var(--border)",
                        borderRadius: 12,
                        background: "var(--panel)",
                        padding: 12,
                    }}
                >
                    <div style={{fontWeight: 600, marginBottom: 8}}>预览</div>
                    {effectItem ? (
                        <div style={{display: "flex", alignItems: "center", gap: 12}}>
                            <AmuletCard item={effectItem} scale={0.75}/>
                            <div style={{color: "var(--text)", lineHeight: 1.6}}>
                                <div>原始ID（raw）：{rawId}</div>
                                <div>显示ID（reg）：{amuletId}</div>
                                <div>Plus：{plus ? "是" : "否"}</div>
                                <div>印章：{badgeId ?? "无"}</div>
                            </div>
                        </div>
                    ) : (
                        <div style={{color: "var(--muted)"}}>请选择护身符以预览。</div>
                    )}
                </div>
            </Modal>

            <AmuletPickerModal
                open={pickA}
                onClose={() => setPickA(false)}
                onSelect={(id) => {
                    setAmuletId(id);
                    setPickA(false);
                }}
            />
            <BadgePickerModal
                open={pickB}
                onClose={() => setPickB(false)}
                onSelect={(id) => {
                    setBadgeId(id);
                    setPickB(false);
                }}
            />
        </>
    );
}