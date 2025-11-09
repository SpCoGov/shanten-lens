import "../styles/theme.css";
import React from "react";
import Modal from "./Modal";
import AmuletPickerModal from "./AmuletPickerModal";
import BadgePickerModal from "./BadgePickerModal";
import AmuletCard from "./AmuletCard";
import {useRegistry} from "../lib/registryStore";
import {useTranslation} from "react-i18next";

export type EditedAmulet = { id: number; plus: boolean; badge: number | null };

export default function AmuletEditorModal({
                                              open,
                                              onClose,
                                              title,
                                              initial,
                                              onConfirm,
                                          }: {
    open: boolean;
    onClose: () => void;
    title?: string;
    initial?: Partial<EditedAmulet>;
    onConfirm: (data: EditedAmulet) => void;
}) {
    const {t} = useTranslation();
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
                {t("amulet_editor.confirm")}
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

    const titleText = title ?? t("amulet_editor.title");

    return (
        <>
            <Modal open={open} onClose={onClose} title={titleText} actions={actions} width={760}>
                <div className="rows" style={{marginBottom: 12}}>
                    <div className="row" style={{gridTemplateColumns: "84px auto 1fr", alignItems: "center"}}>
                        <label>{t("amulet_editor.amuletIdLabel")}</label>
                        <input
                            className="form-input"
                            type="number"
                            value={amuletId ?? ""}
                            onChange={(e) => {
                                const v = e.target.value.trim();
                                setAmuletId(v === "" ? null : Number(v));
                            }}
                            placeholder={t("amulet_editor.amuletIdPlaceholder")}
                            style={{width: 160}}
                        />
                        <div className="toolbar" style={{gap: 8, alignItems: "center", display: "flex"}}>
                            <button className="nav-btn" onClick={() => setPickA(true)}>
                                {t("amulet_editor.pickFromList")}
                            </button>
                            <span
                                className="hint"
                                style={{fontSize: 12, display: "inline-flex", alignItems: "center"}}
                            >
                                {amuletId != null ? (reg ? ` ${reg.name}` : t("amulet_editor.unknownId")) : t("amulet_editor.notSelected")}
                            </span>
                        </div>
                    </div>

                    <div className="row" style={{gridTemplateColumns: "84px auto", alignItems: "center"}}>
                        <label>{t("amulet_editor.plusLabel")}</label>
                        <label style={{display: "inline-flex", alignItems: "center", gap: 8}}>
                            <input className="form-checkbox" type="checkbox" checked={plus} onChange={(e) => setPlus(e.target.checked)} />
                            <span>{t("amulet_editor.plusLabel")}</span>
                        </label>
                    </div>

                    <div className="row" style={{gridTemplateColumns: "84px auto 1fr", alignItems: "center"}}>
                        <label>{t("amulet_editor.badgeLabel")}</label>
                        <input
                            className="form-input"
                            type="number"
                            value={badgeId ?? ""}
                            onChange={(e) => {
                                const v = e.target.value.trim();
                                setBadgeId(v === "" ? null : Number(v));
                            }}
                            placeholder={t("amulet_editor.badgePlaceholder")}
                            style={{width: 160}}
                        />
                        <div className="toolbar" style={{gap: 8, alignItems: "center", display: "flex"}}>
                            <button className="nav-btn" onClick={() => setPickB(true)}>
                                {t("amulet_editor.pickFromList")}
                            </button>
                            <button className="nav-btn" onClick={() => setBadgeId(null)} disabled={badgeId == null}>
                                {t("amulet_editor.clear")}
                            </button>
                            <span
                                className="hint"
                                style={{fontSize: 12, display: "inline-flex", alignItems: "center"}}
                            >
                                {badgeId != null ? badgeById.get(badgeId)?.name ?? t("amulet_editor.unknownBadge") : t("amulet_editor.notSelected")}
                            </span>
                        </div>
                    </div>
                </div>


                <section className="panel">
                    <div className="panel-title">{t("amulet_editor.preview")}</div>
                    {effectItem ? (
                        <div style={{display: "flex", alignItems: "center", gap: 12}}>
                            <AmuletCard item={effectItem} scale={0.75}/>
                            <div style={{lineHeight: 1.6}}>
                                <div>{t("amulet_editor.rawId", {rawId: rawId})}</div>
                                <div>{t("amulet_editor.regId", {regId: amuletId})}</div>
                                <div>{t("amulet_editor.isPlus", {isPlus: plus ? t("amulet_editor.plusYes") : t("amulet_editor.plusNo")})}</div>
                                <div>{t("amulet_editor.badgeId", {badgeId: badgeId ?? t("amulet_editor.badgeValueNone")})}</div>
                            </div>
                        </div>
                    ) : (
                        <div className="hint">{t("amulet_editor.selectToPreview")}</div>
                    )}
                </section>
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