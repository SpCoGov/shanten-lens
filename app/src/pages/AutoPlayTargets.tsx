import React from "react";
import {useTranslation} from "react-i18next";
import type {TargetItem} from "../lib/autoRunnerStore";
import {useRegistry} from "../lib/registryStore";
import AmuletCard from "../components/AmuletCard";
import AmuletEditorModal from "../components/AmuletEditorModal";
import BadgePickerModal from "../components/BadgePickerModal";
import styles from "./AutoPlayConfigPage.module.css";

export default function AutoPlayTargets({targets, onChange}: {targets: TargetItem[]; onChange?: (targets: TargetItem[]) => void}) {
    const {t} = useTranslation();
    const {amuletById, badgeById} = useRegistry();
    const [editor, setEditor] = React.useState<{kind: "amulet" | "badge"; index: number} | null>(null);
    const editing = editor ? targets[editor.index] : undefined;
    function applyTarget(target: TargetItem) {
        if (!editor || !onChange) return;
        const next = [...targets];
        next[editor.index] = {...target, value: editing?.value ?? 1};
        onChange(next); setEditor(null);
    }
    return <>
        <div className={styles.targetList}>{targets.map((target, index) => {
            const amulet = target.kind === "amulet" ? amuletById.get(target.id) : undefined;
            const name = target.kind === "amulet" ? (target.plus ? amulet?.plus_name ?? amulet?.name : amulet?.name)
                ?? t("autoplay.unknown_amulet", {id: target.id}) : badgeById.get(target.id)?.name ?? t("autoplay.unknown_badge", {id: target.id});
            return <article className={`${styles.amulet} ${styles.openingTarget}`} key={index} aria-label={name}>
                {target.kind === "amulet" ? <AmuletCard item={{id: target.id * 10 + (target.plus ? 1 : 0), uid: 0, volume: 1, store: [], tags: [],
                    ...(target.badge ? {badge: {id: target.badge, uid: 0, random: 0, store: []}} : {})}} scale={0.34}/>
                    : <img className={styles.targetBadge} src={`/assets/badge/badge_${target.id}.png`} alt=""/>}
                <div><strong>{name}</strong><span>{target.kind === "amulet"
                    ? target.badge == null ? t("autorun.target_any_badge") : badgeById.get(target.badge)?.name
                    : t("autorun.target_badge_label")}</span></div>
                <div className={styles.targetControls}>
                    {onChange ? <><label>{t("autorun.target_value_label")}<input className="form-input" type="number" min={1} step={1} max={Number.MAX_SAFE_INTEGER}
                        aria-label={t("autoplay.target_value_for", {name})} value={target.value ?? 1}
                        onChange={event => onChange(targets.map((item, i) => i === index ? {...item, value: Number(event.target.value)} : item))}/></label>
                        <div className={styles.targetButtons}>
                            <button className="nav-btn" title={t("autoplay.edit")} aria-label={t("autoplay.edit")} onClick={() => setEditor({kind: target.kind, index})}><span className="ms" aria-hidden="true">edit</span></button>
                            <button className="nav-btn is-danger" title={t("autoplay.remove")} aria-label={t("autoplay.remove")} onClick={() => onChange(targets.filter((_, i) => i !== index))}><span className="ms" aria-hidden="true">delete</span></button>
                        </div></> : <span className={styles.targetWeight}>{t("autorun.target_value_label")} <b>{target.value ?? 1}</b></span>}
                </div>
            </article>;
        })}</div>
        {!targets.length && <p className="hint">{t("autorun.empty_targets")}</p>}
        {onChange && <div className={`${styles.toolbar} ${styles.targetActions}`}>
            <button className="nav-btn" disabled={targets.length >= 128} onClick={() => setEditor({kind: "amulet", index: targets.length})}><span className="ms" aria-hidden="true">add</span>{t("autorun.btn_add_amulet")}</button>
            <button className="nav-btn" disabled={targets.length >= 128} onClick={() => setEditor({kind: "badge", index: targets.length})}><span className="ms" aria-hidden="true">add</span>{t("autorun.btn_add_badge")}</button>
        </div>}
        <AmuletEditorModal open={editor?.kind === "amulet"} onClose={() => setEditor(null)} initial={editing?.kind === "amulet" ? editing : undefined}
            onConfirm={data => applyTarget({kind: "amulet", ...data})}/>
        <BadgePickerModal open={editor?.kind === "badge"} onClose={() => setEditor(null)} onSelect={id => applyTarget({kind: "badge", id})}/>
    </>;
}
