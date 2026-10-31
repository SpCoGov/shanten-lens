import React from "react";
import {useTranslation} from "react-i18next";
import {useAutoRunner, patchAutoConfig, markPlaybookSaved} from "../lib/autoRunnerStore";
import {emptyAutoPlayConfig, newStrategyHand, handFromTargetPreset, newStrategyLoadout, newTacticalStage, moveItem, removeStrategyAmulet,
    getOpeningLevels, getStrategyLevels, validateAutoPlayConfig, type AutoPlayConfig, type PlayProfile, type FullPlayProfile, type OpeningPlayProfile, type TacticalStage,
    type StrategyLoadout, type StrategyAmulet, type StrategyHand} from "../lib/autoPlayConfig";
import {targetKind, targetSize, readTargetPresets, type TargetPreset} from "../lib/customTarget";
import type {EffectItem} from "../lib/gamestate";
import {useRegistry} from "../lib/registryStore";
import {getLevelKey} from "../lib/gameStages";
import {formatLevelIdToLabel} from "../lib/levelFormat";
import * as backendIpc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import AmuletPickerModal from "../components/AmuletPickerModal";
import AmuletEditorModal, {type EditedAmulet} from "../components/AmuletEditorModal";
import AutoPlayTargets from "./AutoPlayTargets";
import AmuletCard from "../components/AmuletCard";
import CustomTargetEditor from "../components/CustomTargetEditor";
import Modal from "../components/Modal";
import styles from "./AutoPlayConfigPage.module.css";

function SortList({ids, onChange, children, label}: {
    ids: string[]; onChange: (ids: string[]) => void; children: (id: string, index: number) => React.ReactNode; label: string;
}) {
    const {t} = useTranslation();
    const [active, setActive] = React.useState<string | null>(null);
    const drag = React.useRef<{id: string; original: string[]} | null>(null);
    function finish(cancel = false) {
        if (cancel && drag.current) onChange(drag.current.original);
        drag.current = null; setActive(null);
    }
    return <div className={styles.sortList} role="list" aria-label={label}
        onPointerMove={event => {
            if (!drag.current) return;
            const row = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-sort-id]");
            if (row && event.currentTarget.contains(row)) {
                const from = ids.indexOf(drag.current.id), to = ids.indexOf(row.dataset.sortId!);
                const rect = row.getBoundingClientRect(), middle = rect.top + rect.height / 2;
                if (from >= 0 && to >= 0 && ((from < to && event.clientY > middle) || (from > to && event.clientY < middle))) {
                    onChange(moveItem(ids, from, to - from));
                }
            }
            const scroll = event.currentTarget.closest<HTMLElement>(".app-main, .modal-body") ?? document.scrollingElement;
            if (scroll) {
                const rect = scroll.getBoundingClientRect();
                if (event.clientY < Math.max(0, rect.top) + 32) scroll.scrollTop -= 18;
                else if (event.clientY > Math.min(window.innerHeight, rect.bottom) - 32) scroll.scrollTop += 18;
            }
        }} onPointerUp={() => finish()} onPointerCancel={() => finish(true)} onLostPointerCapture={() => finish(true)}>
        {ids.map((id, index) => <div className={styles.sortRow} data-sort-id={id} data-dragging={active === id} role="listitem" key={id}>
            <button type="button" className={styles.dragHandle} disabled={ids.length < 2}
                aria-label={t("autoplay.drag_item", {index: index + 1})} title={t("autoplay.drag_hint")}
                onPointerDown={event => {
                    if (event.button !== 0) return;
                    event.preventDefault(); event.currentTarget.focus();
                    drag.current = {id, original: [...ids]}; setActive(id);
                    event.currentTarget.parentElement!.parentElement!.setPointerCapture(event.pointerId);
                }} onKeyDown={event => {
                    if (event.key === "Escape") {finish(true); return;}
                    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
                    event.preventDefault(); onChange(moveItem(ids, index, event.key === "ArrowUp" ? -1 : 1));
                }}><span className="ms" aria-hidden="true">drag_indicator</span></button>
            {children(id, index)}
        </div>)}
    </div>;
}

function cardItem(item: Pick<StrategyAmulet, "id" | "plus"> & Partial<Pick<StrategyAmulet, "badge">>): EffectItem {
    return {id: item.id * 10 + (item.plus ? 1 : 0), uid: 0, volume: 1, store: [], tags: [],
        ...(item.badge ? {badge: {id: item.badge, uid: 0, random: 0, store: []}} : {})};
}

function OpeningTargets({profile, onChange, levels: openingLevels}: {
    profile: OpeningPlayProfile; onChange: (profile: OpeningPlayProfile) => void; levels: ReturnType<typeof getOpeningLevels>;
}) {
    const {t} = useTranslation();
    const cutoff = openingLevels.find(level => level.level === profile.cutoff_level && level.node === profile.cutoff_node);
    return <section className="panel"><h3 className="panel-title">{t("autoplay.opening_targets")}</h3>
        <div className={styles.openingFields}>
            <label className={`${styles.field} ${styles.valueField}`}>{t("autoplay.goal_value")}<input className="form-input" type="number" min={1} step={1} max={Number.MAX_SAFE_INTEGER}
                value={profile.end_count} onChange={event => onChange({...profile, end_count: Number(event.target.value)})}/></label>
            <label className={styles.field}>{t("autoplay.cutoff_level")}<select className="form-input" required value={cutoff ? getLevelKey(cutoff) : ""}
                onChange={event => {
                    const level = openingLevels.find(level => getLevelKey(level) === event.target.value);
                    if (level) onChange({...profile, cutoff_level: level.level, cutoff_node: level.node});
                }}>
                <option value="" disabled>{t("autoplay.select_level")}</option>
                {openingLevels.map(level => <option key={getLevelKey(level)} value={getLevelKey(level)}>{level.label}</option>)}
            </select></label>
        </div>
        <AutoPlayTargets targets={profile.targets} onChange={targets => onChange({...profile, targets})}/>
    </section>;
}

export default function AutoPlayConfigPage() {
    const {t} = useTranslation();
    const {config} = useAutoRunner();
    const {amuletById, badgeById, source} = useRegistry();
    const openingLevels = getOpeningLevels(source), strategyLevels = getStrategyLevels(source);
    const book = config.playbook ?? emptyAutoPlayConfig;
    const profile = book.profiles.find(item => item.id === book.selected_profile_id);
    const fullProfile = profile?.strategy !== "opening" ? profile : undefined;
    const [creating, setCreating] = React.useState(false);
    const [stageId, setStageId] = React.useState("");
    const [stageSection, setStageSection] = React.useState<"conditions" | "loadouts" | "hands">("conditions");
    const stage = fullProfile?.stages.find(item => item.id === stageId) ?? fullProfile?.stages[0];
    const [loadoutId, setLoadoutId] = React.useState("");
    const loadout = stage?.loadouts.find(item => item.id === loadoutId) ?? stage?.loadouts[0];
    const [initialOpen, setInitialOpen] = React.useState(false);
    const [editAmulet, setEditAmulet] = React.useState<StrategyAmulet | "new" | null>(null);
    const [editHand, setEditHand] = React.useState<StrategyHand | null>(null);
    const [handPresets, setHandPresets] = React.useState<TargetPreset[] | null>(null);
    const [handEditorVersion, setHandEditorVersion] = React.useState(0);
    const [transfer, setTransfer] = React.useState<"import" | "export" | null>(null);
    const [transferText, setTransferText] = React.useState("");
    const [transferError, setTransferError] = React.useState("");
    const [saving, setSaving] = React.useState(false);

    function change(next: AutoPlayConfig) {
        patchAutoConfig({playbook: next});
    }
    function replaceProfile(next: PlayProfile) {
        change({...book, profiles: book.profiles.map(item => item.id === next.id ? next : item)});
    }
    function changeProfile(patch: Partial<FullPlayProfile>) {
        if (fullProfile) replaceProfile({...fullProfile, ...patch});
    }
    function changeStage(patch: Partial<TacticalStage>) {
        if (fullProfile && stage) changeProfile({stages: fullProfile.stages.map(item => item.id === stage.id ? {...stage, ...patch} : item)});
    }
    function changeLoadout(next: StrategyLoadout) {
        if (stage && loadout) changeStage({loadouts: stage.loadouts.map(item => item.id === loadout.id ? next : item)});
    }
    function addProfile(strategy: "opening" | "full") {
        const common = {id: crypto.randomUUID(), name: t("autoplay.default_profile", {count: book.profiles.length + 1})};
        const next: PlayProfile = strategy === "opening" ? {...common, strategy, end_count: 1, targets: [], cutoff_level: 0, cutoff_node: 0} : {...common, strategy,
            character_id: 201, initial_amulet_id: 0, first_level_min_coin: "0",
            stages: [newTacticalStage(t("autoplay.default_stage", {count: 1}), t("autoplay.default_loadout", {count: 1}), t("autoplay.default_hand", {count: 1}), strategyLevels[0])]};
        change({...book, selected_profile_id: next.id, profiles: [...book.profiles, next]});
        setCreating(false); setStageId(""); setLoadoutId("");
    }
    function addAmulet(data: EditedAmulet) {
        if (!loadout) return;
        if (editAmulet && editAmulet !== "new") {
            const badge = editAmulet.badge === 0 && data.badge === null ? 0 : data.badge;
            changeLoadout({...loadout, amulets: loadout.amulets.map(item => item.key === editAmulet.key ? {...item, ...data, badge} : item)});
        } else {
            const item = {...data, key: crypto.randomUUID()};
            changeLoadout({...loadout, amulets: [...loadout.amulets, item], entry_order: [...loadout.entry_order, item.key], win_order: [...loadout.win_order, item.key]});
        }
        setEditAmulet(null);
    }
    function amuletName(item: Pick<StrategyAmulet, "id" | "plus">) {
        const amulet = amuletById.get(item.id);
        return (item.plus ? amulet?.plus_name ?? amulet?.name : amulet?.name) ?? t("autoplay.unknown_amulet", {id: item.id});
    }
    function badgeName(item: StrategyAmulet) {
        return item.badge === null ? t("autoplay.any_badge") : item.badge === 0 ? t("autoplay.no_badge")
            : badgeById.get(item.badge)?.name ?? t("autoplay.unknown_badge", {id: item.badge});
    }
    function orderEditor(field: "entry_order" | "win_order") {
        if (!loadout) return null;
        return <section className={styles.order}><h4>{t(`autoplay.${field}`)}</h4>
            <SortList ids={loadout[field]} label={t(`autoplay.${field}`)} onChange={order => changeLoadout({...loadout, [field]: order})}>{key => {
                const item = loadout.amulets.find(item => item.key === key)!;
                const slot = loadout.amulets.indexOf(item) + 1;
                return <><AmuletCard item={cardItem(item)} scale={0.23}/><span className={styles.orderName}>{t("autoplay.slot", {index: slot})} · {amuletName(item)}<small>{badgeName(item)}</small></span></>;
            }}</SortList>
        </section>;
    }
    function openHand(hand: StrategyHand, presets = false) {
        setEditHand(structuredClone(hand)); setHandPresets(presets ? readTargetPresets() : null);
        setHandEditorVersion(value => value + 1);
    }
    async function save() {
        const invalid = validateAutoPlayConfig(book, amuletById, badgeById);
        if (invalid) {pushToast(t(`autoplay.error_${invalid}`), "error", 5000); return;}
        setSaving(true);
        try {
            await backendIpc.updateConfig({autorun: {playbook: book}});
            markPlaybookSaved(book); pushToast(t("autoplay.saved"), "success");
        } catch { pushToast(t("autoplay.save_failed"), "error", 5000); }
        finally { setSaving(false); }
    }
    function importBook() {
        try {
            const next: unknown = JSON.parse(transferText);
            const invalid = validateAutoPlayConfig(next, amuletById, badgeById);
            if (invalid) {setTransferError(t(`autoplay.error_${invalid}`)); return;}
            change(next as AutoPlayConfig); setTransfer(null); setStageId(""); setLoadoutId("");
        } catch { setTransferError(t("autoplay.error_format")); }
    }
    const validHand = editHand && !!editHand.name.trim() && targetSize(editHand.target_groups) === 14 && editHand.target_groups.filter(group => targetKind(group) === "quad").length <= 4;

    return <div className={styles.page}>
        <fieldset className={styles.workspace} disabled={saving}>
            <aside className={`panel ${styles.profileList}`}>
                <div className={styles.profileHeading}><h3 className="panel-title" id="autoplay-profile-label">{t("autoplay.profile")}</h3><span className="badge">{book.profiles.length}</span></div>
                <div className={styles.profileItems} role="group" aria-labelledby="autoplay-profile-label">
                    {book.profiles.map(item => <button className={styles.profileItem} key={item.id} aria-pressed={item.id === book.selected_profile_id}
                        onClick={() => {change({...book, selected_profile_id: item.id}); setStageId(""); setLoadoutId("");}}>
                        <span className="ms" aria-hidden="true">{item.strategy === "opening" ? "restart_alt" : "route"}</span>
                        <span><strong>{item.name}</strong><small>{t(`autoplay.strategy_${item.strategy ?? "full"}`)} · {item.strategy === "opening"
                            ? t("autoplay.target_count", {count: item.targets.length}) : t("autoplay.stage_count", {count: item.stages.length})}</small></span>
                        <span className="ms" aria-hidden="true">chevron_right</span>
                    </button>)}
                    {!book.profiles.length && <p className="hint">{t("autoplay.no_profiles")}</p>}
                </div>
                <button className="nav-btn" onClick={() => setCreating(true)} disabled={book.profiles.length >= 32}><span className="ms" aria-hidden="true">add</span>{t("autoplay.add_profile")}</button>
            </aside>
            <div className={styles.content}>
            <section className={`panel ${styles.profileEditor}`}>
                {profile && <label className={styles.field}>{t("autoplay.profile_name")}<input className="form-input" maxLength={80} value={profile.name}
                    onChange={event => replaceProfile({...profile, name: event.target.value})}/></label>}
                <div className={styles.profileActions}>
                    {profile && <>
                    <span className={styles.strategyTag}><span className="ms" aria-hidden="true">{profile.strategy === "opening" ? "restart_alt" : "route"}</span>{t(`autoplay.strategy_${profile.strategy ?? "full"}`)}</span>
                    <button className="nav-btn is-danger" title={t("autoplay.delete_profile")} aria-label={t("autoplay.delete_profile")} onClick={() => {
                        const profiles = book.profiles.filter(item => item.id !== profile.id);
                        change({...book, profiles, selected_profile_id: profiles[0]?.id ?? ""});
                    }}><span className="ms" aria-hidden="true">delete</span></button>
                    <button className="nav-btn" title={t("autoplay.duplicate")} aria-label={t("autoplay.duplicate")} disabled={book.profiles.length >= 32} onClick={() => {
                        const copy = {...structuredClone(profile), id: crypto.randomUUID(), name: t("autoplay.copy_name", {name: profile.name}).slice(0, 80)};
                        change({...book, selected_profile_id: copy.id, profiles: [...book.profiles, copy]});
                    }}><span className="ms" aria-hidden="true">content_copy</span></button>
                    </>}
                    <button className="nav-btn" title={t("autoplay.import")} aria-label={t("autoplay.import")}
                        onClick={() => {setTransferText(""); setTransferError(""); setTransfer("import");}}><span className="ms" aria-hidden="true">upload</span></button>
                    <button className="nav-btn" title={t("autoplay.export")} aria-label={t("autoplay.export")}
                        onClick={() => {setTransferText(JSON.stringify(book, null, 2)); setTransfer("export");}}><span className="ms" aria-hidden="true">download</span></button>
                    <button className={`nav-btn ${styles.primary}`} title={t(saving ? "autoplay.saving" : "autoplay.save")} aria-label={t(saving ? "autoplay.saving" : "autoplay.save")}
                        onClick={() => void save()} disabled={!amuletById.size}><span className="ms" aria-hidden="true">save</span></button>
                </div>
            </section>
            {!profile && <div className={`panel ${styles.empty}`}><span className="ms" aria-hidden="true">route</span><p>{t("autoplay.empty")}</p></div>}
            {profile?.strategy === "opening" && <OpeningTargets levels={openingLevels} key={profile.id} profile={profile} onChange={replaceProfile}/>}
            {profile && profile.strategy !== "opening" && <>
                <section className="panel"><h3 className="panel-title">{t("autoplay.opening")}</h3>
                    <div className={styles.opening}>
                        <div><div className={styles.label}>{t("autoplay.character")}</div><div className={styles.characters}>
                            {[201, 202, 203, 204].map(id => <button className={styles.character} key={id} aria-pressed={profile.character_id === id}
                                onClick={() => changeProfile({character_id: id})} aria-label={t("character_health.portrait_alt", {id})}>
                                <img src={`/assets/character/character_${id}.png`} alt=""/><span>{t("character_health.portrait_alt", {id})}</span>
                            </button>)}
                        </div></div>
                        <div className={styles.openingDetails}><div className={styles.field}><span>{t("autoplay.initial_amulet")}</span>
                            <div className={styles.initialAmulet}>
                                {!!profile.initial_amulet_id && <AmuletCard item={cardItem({id: profile.initial_amulet_id, plus: false})} scale={0.48} onClick={() => setInitialOpen(true)}/>}
                                <div><button className="nav-btn" onClick={() => setInitialOpen(true)}><span className="ms" aria-hidden="true">{profile.initial_amulet_id ? "swap_horiz" : "add"}</span>{t(profile.initial_amulet_id ? "autoplay.change_amulet" : "autoplay.select_amulet")}</button>
                                    {!!profile.initial_amulet_id && <strong>{amuletName({id: profile.initial_amulet_id, plus: false})}</strong>}
                                    <p className="hint">{t("autoplay.initial_hint")}</p></div>
                            </div></div>
                            <label className={styles.field}>{t("autoplay.coin_deadline")}
                                <input className="form-input" inputMode="numeric" maxLength={128} value={profile.first_level_min_coin}
                                    onChange={event => changeProfile({first_level_min_coin: event.target.value})}/>
                                <span className="hint">{t("autoplay.coin_hint")}</span></label>
                        </div>
                    </div>
                </section>
                <section className={styles.stageLayout}>
                    <aside className={`panel ${styles.stageList}`}><h3 className="panel-title">{t("autoplay.stages")}</h3>
                        <SortList ids={profile.stages.map(item => item.id)} label={t("autoplay.stages")}
                            onChange={ids => {if (stage) setStageId(stage.id); changeProfile({stages: ids.map(id => profile.stages.find(item => item.id === id)!)});}}>{(id, index) => {
                            const item = profile.stages.find(item => item.id === id)!;
                            return <button className={styles.stageButton} aria-pressed={item.id === stage?.id}
                            onClick={() => {setStageId(item.id); setLoadoutId("");}}><span className="ms" aria-hidden="true">flag</span><strong>{index + 1}. {item.name}</strong>
                                <small>{t("autoplay.stage_summary", {start: formatLevelIdToLabel(item.start_level, item.start_node === 0 ? "qyzz" : "packet", item.start_node), count: item.loadouts.length})}</small></button>;
                        }}</SortList>
                        <button className="nav-btn" disabled={profile.stages.length >= 32} onClick={() => {
                            const last = profile.stages[profile.stages.length - 1];
                            const index = strategyLevels.findIndex(level => level.level === last?.start_level && level.node === last?.start_node);
                            const next = newTacticalStage(t("autoplay.default_stage", {count: profile.stages.length + 1}), t("autoplay.default_loadout", {count: 1}),
                                t("autoplay.default_hand", {count: 1}), strategyLevels[Math.min(index + 1, strategyLevels.length - 1)]);
                            changeProfile({stages: [...profile.stages, next]}); setStageId(next.id);
                        }}><span className="ms" aria-hidden="true">add</span>{t("autoplay.add_stage")}</button>
                    </aside>
                    {stage && <div className={styles.stageContent} key={stage.id}>
                        <div className={styles.choices} role="group" aria-label={t("autoplay.stages")}>
                            {(["conditions", "loadouts", "hands"] as const).map(section => <button className="nav-btn" key={section} aria-pressed={stageSection === section} onClick={() => setStageSection(section)}>
                                <span className="ms" aria-hidden="true">{section === "conditions" ? "flag" : section === "loadouts" ? "layers" : "view_module"}</span>{t(`autoplay.${section === "conditions" ? "stage_settings" : section}`)}</button>)}
                        </div>
                        {stageSection === "conditions" && <section className="panel"><div className={styles.toolbar}><h3 className="panel-title">{t("autoplay.stage_settings")}</h3>
                            <button className="nav-btn is-danger" disabled={profile.stages.length <= 1} onClick={() => changeProfile({stages: profile.stages.filter(item => item.id !== stage.id)})}><span className="ms" aria-hidden="true">delete</span>{t("autoplay.delete_stage")}</button>
                        </div>
                            <label className={styles.field}>{t("autoplay.stage_name")}<input className="form-input" maxLength={80} value={stage.name} onChange={event => changeStage({name: event.target.value})}/></label>
                            <div className={styles.fields}>{(["start", "deadline"] as const).map(field => <label className={styles.field} key={field}>{t(`autoplay.${field}_level`)}
                                <select className="form-input" value={strategyLevels.some(level => level.level === stage[`${field}_level`] && level.node === stage[`${field}_node`])
                                    ? getLevelKey({level: stage[`${field}_level`], node: stage[`${field}_node`]}) : ""} onChange={event => {
                                    const level = strategyLevels.find(level => getLevelKey(level) === event.target.value)!;
                                    changeStage({[`${field}_level`]: level.level, [`${field}_node`]: level.node});
                                }}>
                                    <option value="" disabled>{t("autoplay.select_level")}</option>
                                    {strategyLevels.map(level => <option key={getLevelKey(level)} value={getLevelKey(level)}>{level.label}</option>)}
                                </select></label>)}</div>
                            <p className="hint">{t("autoplay.deadline_hint")}</p>
                            <label className={styles.check}><input type="checkbox" checked={stage.enter_early_if_unreachable}
                                onChange={event => changeStage({enter_early_if_unreachable: event.target.checked})}/>{t("autoplay.early_entry")}</label>
                            <p className="hint">{t("autoplay.early_hint")}</p>
                        </section>}
                        {stageSection === "loadouts" && <section className="panel"><div className={styles.toolbar}><h3 className="panel-title">{t("autoplay.loadouts")}</h3>
                            <button className="nav-btn" disabled={stage.loadouts.length >= 16} onClick={() => {
                                const next = newStrategyLoadout(t("autoplay.default_loadout", {count: stage.loadouts.length + 1}));
                                changeStage({loadouts: [...stage.loadouts, next]}); setLoadoutId(next.id);
                            }}><span className="ms" aria-hidden="true">add</span>{t("autoplay.add_loadout")}</button></div>
                            <p className="hint">{t("autoplay.loadout_hint")}</p>
                            <div className={styles.tabs}>{stage.loadouts.map(item => <button className="nav-btn" key={item.id} aria-pressed={item.id === loadout?.id}
                                onClick={() => setLoadoutId(item.id)}><span className="ms" aria-hidden="true">layers</span>{item.name}</button>)}</div>
                            {loadout && <div className={styles.loadout} key={loadout.id}>
                                <div className={`${styles.toolbar} ${styles.controls}`}><label className={styles.field}>{t("autoplay.loadout_name")}<input className="form-input" maxLength={80} value={loadout.name}
                                    onChange={event => changeLoadout({...loadout, name: event.target.value})}/></label>
                                    <button className="nav-btn" disabled={stage.loadouts.length >= 16} onClick={() => {
                                        const copy = {...structuredClone(loadout), id: crypto.randomUUID(), name: t("autoplay.copy_name", {name: loadout.name}).slice(0, 80)};
                                        changeStage({loadouts: [...stage.loadouts, copy]}); setLoadoutId(copy.id);
                                    }}><span className="ms" aria-hidden="true">content_copy</span>{t("autoplay.duplicate")}</button>
                                    <button className="nav-btn is-danger" disabled={stage.loadouts.length <= 1} onClick={() => changeStage({loadouts: stage.loadouts.filter(item => item.id !== loadout.id)})}><span className="ms" aria-hidden="true">delete</span>{t("autoplay.delete_loadout")}</button>
                                </div>
                                <div className={styles.amulets}>{loadout.amulets.map((item, index) => <article className={styles.amulet} key={item.key}>
                                    <AmuletCard item={cardItem(item)} scale={0.34}/>
                                    <div><small>{t("autoplay.slot", {index: index + 1})}</small><strong>{amuletName(item)}</strong><span>{badgeName(item)}</span></div>
                                    <div className={styles.amuletActions}><button className="nav-btn" onClick={() => setEditAmulet(item)}><span className="ms" aria-hidden="true">edit</span>{t("autoplay.edit")}</button>
                                        <button className="nav-btn" disabled={loadout.amulets.length >= 32} onClick={() => {
                                            const copy = {...item, key: crypto.randomUUID()};
                                            changeLoadout({...loadout, amulets: [...loadout.amulets, copy], entry_order: [...loadout.entry_order, copy.key], win_order: [...loadout.win_order, copy.key]});
                                        }}><span className="ms" aria-hidden="true">content_copy</span>{t("autoplay.add_copy")}</button>
                                        <button className="nav-btn is-danger" onClick={() => changeLoadout(removeStrategyAmulet(loadout, item.key))}><span className="ms" aria-hidden="true">delete</span>{t("autoplay.remove")}</button>
                                    </div>
                                    <label className={styles.check}><input type="checkbox" checked={item.badge === 0} onChange={event => changeLoadout({...loadout,
                                        amulets: loadout.amulets.map(a => a.key === item.key ? {...a, badge: event.target.checked ? 0 : null} : a)})}/>{t("autoplay.require_no_badge")}</label>
                                </article>)}</div>
                                <button className="nav-btn" disabled={loadout.amulets.length >= 32} onClick={() => setEditAmulet("new")}><span className="ms" aria-hidden="true">add</span>{t("autoplay.add_amulet")}</button>
                                <p className="hint">{t("autoplay.order_hint")}</p>
                                <div className={styles.orders}>{orderEditor("entry_order")}{orderEditor("win_order")}</div>
                            </div>}
                        </section>}
                        {stageSection === "hands" && <section className="panel"><div className={styles.toolbar}><h3 className="panel-title">{t("autoplay.hands")}</h3>
                            <button className="nav-btn" disabled={stage.hands.length >= 32} onClick={() => openHand(
                                newStrategyHand(t("autoplay.default_hand", {count: stage.hands.length + 1})), true)}><span className="ms" aria-hidden="true">bookmarks</span>{t("autoplay.reuse_hand")}</button>
                            <button className="nav-btn" disabled={stage.hands.length >= 32} onClick={() => openHand(
                                newStrategyHand(t("autoplay.default_hand", {count: stage.hands.length + 1})))}><span className="ms" aria-hidden="true">add</span>{t("autoplay.add_hand")}</button></div>
                            <div className={styles.field}><span id="autoplay-hand-mode">{t("autoplay.hand_selection")}</span>
                                <div className={styles.choices} role="group" aria-labelledby="autoplay-hand-mode">{(["most_wins", "priority"] as const).map(mode =>
                                    <button className="nav-btn" key={mode} aria-pressed={stage.hand_selection === mode} onClick={() => changeStage({hand_selection: mode})}>
                                        <span className="ms" aria-hidden="true">{mode === "most_wins" ? "leaderboard" : "format_list_numbered"}</span>{t(`autoplay.${mode}`)}</button>)}
                                </div></div><p className="hint">{t("autoplay.hand_hint")}</p>
                            <SortList ids={stage.hands.map(hand => hand.id)} label={t("autoplay.hands")}
                                onChange={ids => changeStage({hands: ids.map(id => stage.hands.find(hand => hand.id === id)!)})}>{(id, index) => {
                                const hand = stage.hands.find(hand => hand.id === id)!;
                                return <div className={`${styles.toolbar} ${styles.controls} ${styles.handRow}`}>
                                <label className={styles.field}>{t("autoplay.hand_name", {index: index + 1})}<input className="form-input" maxLength={80} value={hand.name}
                                    onChange={event => changeStage({hands: stage.hands.map(item => item.id === hand.id ? {...hand, name: event.target.value} : item)})}/></label>
                                    <button className="nav-btn" onClick={() => openHand(hand)}><span className="ms" aria-hidden="true">tune</span>{t("autoplay.edit_hand")}</button>
                                    <button className="nav-btn is-danger" disabled={stage.hands.length <= 1} onClick={() => changeStage({hands: stage.hands.filter(item => item.id !== hand.id)})}>
                                        <span className="ms" aria-hidden="true">delete</span>{t("autoplay.remove")}</button>
                                </div>;
                            }}</SortList>
                        </section>}
                    </div>}
                </section>
            </>}
            </div>
        </fieldset>
        <Modal open={creating} onClose={() => setCreating(false)} title={t("autoplay.choose_strategy")} width={640} className={styles.dialog}>
            <div className={styles.strategyChoices}>{(["opening", "full"] as const).map(strategy => <button className={styles.strategyChoice} key={strategy} onClick={() => addProfile(strategy)}>
                <span className="ms" aria-hidden="true">{strategy === "opening" ? "restart_alt" : "route"}</span>
                <strong>{t(`autoplay.strategy_${strategy}`)}</strong><span>{t(`autoplay.strategy_${strategy}_hint`)}</span>
            </button>)}</div>
        </Modal>
        <AmuletPickerModal open={initialOpen} onClose={() => setInitialOpen(false)} allowedRarities={["BLUE", "GREEN"]}
            onSelect={id => changeProfile({initial_amulet_id: id})}/>
        <AmuletEditorModal open={editAmulet !== null} onClose={() => setEditAmulet(null)}
            initial={editAmulet && editAmulet !== "new" ? {...editAmulet, badge: editAmulet.badge || null} : undefined} onConfirm={addAmulet}/>
        <Modal open={editHand !== null} onClose={() => setEditHand(null)} title={t("autoplay.edit_hand")} width={1280} className={styles.dialog}
            actions={<button className="nav-btn" disabled={!validHand} onClick={() => {
                if (editHand && stage) changeStage({hands: stage.hands.some(hand => hand.id === editHand.id)
                    ? stage.hands.map(hand => hand.id === editHand.id ? editHand : hand) : [...stage.hands, editHand]});
                setEditHand(null);
            }}><span className="ms" aria-hidden="true">check</span>{t("autoplay.apply_hand")}</button>}>
            {editHand && <div className={styles.handEditor}>
                <div className={styles.toolbar}><button className="nav-btn" aria-expanded={handPresets !== null} onClick={() => setHandPresets(handPresets === null ? readTargetPresets() : null)}>
                    <span className="ms" aria-hidden="true">bookmarks</span>{t("autoplay.reuse_hand")}</button><p className="hint">{t("autoplay.preset_copy_hint")}</p></div>
                {handPresets !== null && <div className={styles.presetList} role="group" aria-label={t("custom_target.preset_saved_list")}>
                    {handPresets.length ? handPresets.map((preset, index) => <button className="nav-btn" key={`${preset.name}-${index}`} onClick={() => {
                        setEditHand({...handFromTargetPreset(preset), id: editHand.id}); setHandEditorVersion(value => value + 1); setHandPresets(null);
                    }}><span className="ms" aria-hidden="true">bookmark</span>{preset.name}</button>) : <p className="hint">{t("autoplay.no_hand_presets")}</p>}
                </div>}
                <label className={styles.field}>{t("autoplay.hand_title")}<input className="form-input" maxLength={80} value={editHand.name}
                    onChange={event => setEditHand({...editHand, name: event.target.value})}/></label>
                <label className={styles.check}><input type="checkbox" checked={editHand.exclude_invalid_shapes}
                onChange={event => setEditHand({...editHand, exclude_invalid_shapes: event.target.checked})}/>{t("custom_target.exclude_invalid_shapes")}</label>
                <CustomTargetEditor key={handEditorVersion} groups={editHand.target_groups} onChange={target_groups => setEditHand({...editHand, target_groups})}/>
            </div>}
        </Modal>
        <Modal open={transfer !== null} onClose={() => setTransfer(null)} title={t(transfer === "import" ? "autoplay.import" : "autoplay.export")} width={720} className={styles.dialog}>
            <div className={styles.loadout}><p className="hint">{t(transfer === "import" ? "autoplay.import_hint" : "autoplay.export_hint")}</p>
                <textarea className={`form-input ${styles.transfer}`} rows={14} maxLength={1024 * 1024} value={transferText} readOnly={transfer === "export"}
                    aria-label={t("autoplay.config_text")} onChange={event => {setTransferText(event.target.value); setTransferError("");}}/>
                {transferError && <p className={styles.error} role="alert">{transferError}</p>}
                {transfer === "import" && <button className="nav-btn" disabled={!transferText.trim() || !amuletById.size} onClick={importBook}><span className="ms" aria-hidden="true">check</span>{t("autoplay.apply_import")}</button>}
            </div>
        </Modal>
    </div>;
}
