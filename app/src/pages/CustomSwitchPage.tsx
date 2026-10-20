import React from "react";
import {useTranslation} from "react-i18next";
import Tile from "../components/Tile";
import Modal from "../components/Modal";
import type {GameStateData} from "../lib/gamestate";
import type {PlanData} from "../lib/planTypes";
import {newTargetGroup, targetSize, targetKind, targetGroupSize, TARGET_KINDS, restoreTargetGroups, applyTargetRule, parseTargetPreset, compactTargetPreset, type TargetGroup, type TargetRule, type TargetKind, type TargetPreset} from "../lib/customTarget";
import * as backendIpc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import styles from "./CustomSwitchPage.module.css";

const STORAGE_KEY = "sl-custom-switch:target-v1";
const EXCLUDE_INVALID_SHAPES_KEY = "sl-custom-switch:exclude-invalid-shapes";
const PRESETS_KEY = "sl-custom-switch:presets-v1";
const SUITS = ["m", "p", "s", "z"];
const LINK_COLORS = ["red", "orange", "yellow", "green", "cyan", "blue", "purple"];

function readTarget(): TargetGroup[] {
    try {
        return restoreTargetGroups(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null"));
    } catch {}
    return Array.from({length: 14}, newTargetGroup);
}

function readPresets(): TargetPreset[] {
    try {
        const saved = JSON.parse(localStorage.getItem(PRESETS_KEY) ?? "[]");
        return Array.isArray(saved) ? saved.map(parseTargetPreset).filter((preset): preset is TargetPreset => preset !== null) : [];
    } catch { return []; }
}

export default function CustomSwitchPage({currentState, data, onClear}: {
    currentState: GameStateData | null;
    data: PlanData | null;
    onClear: () => void;
}) {
    const {t} = useTranslation();
    const [groups, setGroups] = React.useState(readTarget);
    const [selected, setSelected] = React.useState<number[]>(() => groups.length ? [0] : []);
    const [multiSelect, setMultiSelect] = React.useState(false);
    const [wallLimit, setWallLimit] = React.useState(36);
    const [executing, setExecuting] = React.useState(false);
    const [excludeInvalidShapes, setExcludeInvalidShapes] = React.useState(() => {
        try { return localStorage.getItem(EXCLUDE_INVALID_SHAPES_KEY) === "1"; } catch { return false; }
    });
    const [presets, setPresets] = React.useState(readPresets);
    const [presetName, setPresetName] = React.useState("");
    const [importOpen, setImportOpen] = React.useState(false);
    const [importText, setImportText] = React.useState("");
    const [importError, setImportError] = React.useState("");
    const searching = data?.status === "searching";
    const size = targetSize(groups);
    const quads = groups.filter(group => targetKind(group) === "quad").length;
    const active = groups[selected[0]];
    const ready = currentState && [4, 5].includes(currentState.stage);
    const stale = data?.status === "plan" && data.state_revision !== currentState?.revision;
    const canExecute = ready && data?.status === "plan" && data.request_source === "live"
        && data.search_algorithm === "custom_target" && !!data.plan_id && !stale && !executing;
    const valid = size === 14 && quads <= 4;
    const hasJoker = currentState?.hand_tiles.some(id => currentState.deck_map[String(id)] === "bd") ?? false;
    React.useEffect(() => {
        try { localStorage.setItem(STORAGE_KEY, JSON.stringify(groups)); } catch {}
    }, [groups]);
    React.useEffect(() => {
        try { localStorage.setItem(EXCLUDE_INVALID_SHAPES_KEY, excludeInvalidShapes ? "1" : "0"); } catch {}
    }, [excludeInvalidShapes]);
    React.useEffect(() => {
        if (searching && !ready) void backendIpc.runSwitch({action: "stop", notify: false});
    }, [searching, ready]);

    function clearResult() {
        if (searching) void backendIpc.runSwitch({action: "stop", notify: false});
        onClear();
    }
    async function execute(action: "execute_plan" | "execute_full_plan") {
        if (!canExecute) return;
        setExecuting(true);
        try {
            await backendIpc.runSwitch({action, options: {plan_id: data!.plan_id!}});
        } finally {
            setExecuting(false);
        }
    }
    function changeGroups(next: TargetGroup[]) {
        clearResult();
        setGroups(next);
        setSelected(indices => indices.filter(index => index < next.length));
    }
    function currentPreset(): TargetPreset {
        return {format: "shanten-lens.custom-target", version: 1, name: presetName.trim() || t("custom_target.preset_default_name"),
            target_groups: groups, wall_limit: wallLimit, exclude_invalid_shapes: excludeInvalidShapes};
    }
    function storePresets(next: TargetPreset[]): boolean {
        try {
            localStorage.setItem(PRESETS_KEY, JSON.stringify(next.map(compactTargetPreset)));
            setPresets(next);
            return true;
        } catch {
            pushToast(t("custom_target.preset_save_failed"), "error");
            return false;
        }
    }
    function savePreset() {
        if (!presetName.trim()) return;
        const preset = currentPreset();
        if (storePresets([preset, ...presets.filter(item => item.name !== preset.name)])) {
            setPresetName(preset.name);
            pushToast(t("custom_target.preset_saved", {name: preset.name}), "success");
        }
    }
    function loadPreset(preset: TargetPreset) {
        changeGroups(restoreTargetGroups(preset.target_groups));
        setSelected(preset.target_groups.length ? [0] : []);
        setMultiSelect(false);
        setWallLimit(preset.wall_limit);
        setExcludeInvalidShapes(preset.exclude_invalid_shapes);
        setPresetName(preset.name);
    }
    async function exportPreset() {
        try {
            await navigator.clipboard.writeText(JSON.stringify(compactTargetPreset(currentPreset()), null, 2));
            pushToast(t("custom_target.preset_copied"), "success");
        } catch { pushToast(t("custom_target.preset_export_failed"), "error"); }
    }
    function importPreset() {
        try {
            if (importText.length > 1024 * 1024) throw new Error("preset-text-too-large");
            const preset = parseTargetPreset(JSON.parse(importText));
            if (!preset) throw new Error("invalid-target-preset");
            loadPreset(preset);
            setImportOpen(false);
            pushToast(t("custom_target.preset_imported"), "success");
        } catch { setImportError(t("custom_target.preset_import_failed")); }
    }
    function removeGroups(indices: number[]) {
        const next = groups.filter((_, index) => !indices.includes(index));
        const nextSelected = selected.filter(index => !indices.includes(index))
            .map(index => index - indices.filter(removed => removed < index).length);
        changeGroups(next);
        setSelected(nextSelected.length ? nextSelected : next.length ? [Math.min(indices[0], next.length - 1)] : []);
    }
    function changeRule(patch: Partial<TargetRule> | ((rule: TargetRule) => Partial<TargetRule>)) {
        if (active) changeGroups(applyTargetRule(groups, selected, patch));
    }
    function changeKind(kind: TargetKind) {
        changeGroups(groups.map((group, index) => selected.includes(index)
            ? {...group, kind, quad: kind === "quad", rule: {...group.rule, joker: kind === "quad" ? "exclude" : "allow"}}
            : group));
    }
    function changeLink(key: "suit_group" | "distinct_group", value: string) {
        changeGroups(groups.map((group, index) => selected.includes(index) ? {...group, [key]: value} : group));
    }
    function linkOptions(key: "suit_group" | "distinct_group") {
        return [...new Set([...LINK_COLORS, ...groups.map(group => group[key]?.trim()).filter((value): value is string => !!value)])];
    }
    function linkName(value: string) {
        const index = LINK_COLORS.indexOf(value);
        return index >= 0 ? t("custom_target.link_numbered", {number: index + 1, name: t(`custom_target.link_color_${value}`)}) : value;
    }
    function toggle<T>(values: T[], value: T, enabled = !values.includes(value)): T[] {
        return enabled ? values.includes(value) ? values : [...values, value] : values.filter(item => item !== value);
    }
    function description(group: TargetGroup) {
        const {rule} = group;
        return [rule.faces.join(" "), rule.suits.map(suit => t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)).join("/"),
            rule.ranks.join("/"), ...(["red", "dora", "soul"] as const).filter(key => rule[key]).map(key => t(`custom_target.${key}`)),
            group.suit_group?.trim() ? t("custom_target.suit_link_label", {name: linkName(group.suit_group.trim())}) : "",
            group.distinct_group?.trim() ? t("custom_target.distinct_link_label", {name: linkName(group.distinct_group.trim())}) : ""]
            .filter(Boolean).join(" · ") || t("custom_target.any");
    }
    function groupLabel(group: Pick<TargetGroup, "quad" | "kind">, index: number) {
        return t("custom_target.kind_slot", {index: index + 1, kind: t(`custom_target.kind_${targetKind(group)}`)});
    }
    function tiles(ids: number[]) {
        return <div className={styles.tiles}>{ids.map(id => <span key={id} title={t("advisor.id_label", {id})}>
            {currentState?.deck_map[String(id)]
                ? <Tile tile={currentState.deck_map[String(id)]} width={32} height={44}/>
                : t("advisor.id_label", {id})}
        </span>)}</div>;
    }
    const reason = data?.reason ?? "unknown";
    const progress = data?.search_progress;

    return <div className={`settings-wrap wide-page switch-guide-page ${styles.page}`}>
        <section className={`panel switch-guide-hero ${styles.hero}`}>
            <div className="switch-guide-heading">
                <div className={styles.heading}>
                    <span className={`ms ${styles.heroIcon}`} aria-hidden="true">dashboard_customize</span>
                    <div><h1>{t("custom_target.title")}</h1><p>{t("custom_target.intro")}</p></div>
                </div>
                <span className={`switch-guide-status ${ready ? "is-ready" : ""}`}>
                    <span aria-hidden="true"/>{t(ready ? "custom_target.ready" : "custom_target.waiting")}
                </span>
            </div>
            <div className="switch-guide-command-bar">
                <div className="switch-guide-actions">
                    <label className={styles.wallLimit}>{t("blackhole.wall_limit")}
                        <input className="form-input" type="number" min={0} max={36} value={wallLimit}
                            onChange={event => {clearResult(); setWallLimit(Math.min(36, Math.max(0, Math.trunc(Number(event.target.value) || 0))));}}/>
                        <span>{t("custom_target.wall_hint")}</span>
                    </label>
                    {searching ? <button className="switch-guide-button is-danger" onClick={() => void backendIpc.runSwitch({action: "stop"})}>
                        <span className="ms" aria-hidden="true">stop_circle</span>{t("advisor.stop_search")}
                    </button> : <button className="switch-guide-button is-primary" disabled={!ready || !valid || executing} onClick={() => {
                        onClear();
                        void backendIpc.runSwitch({action: "start", options: {search_algorithm: "custom_target", target_groups: groups,
                            wall_limit: wallLimit, exclude_invalid_shapes: excludeInvalidShapes}});
                    }}><span className="ms" aria-hidden="true">manage_search</span>{t("custom_target.start")}</button>}
                    <button className="switch-guide-button is-success" disabled={!canExecute} onClick={() => void execute("execute_plan")}>
                        <span className="ms" aria-hidden="true">play_arrow</span>{t("blackhole.execute_plan")}
                    </button>
                    <button className="switch-guide-button is-success" disabled={!canExecute} onClick={() => void execute("execute_full_plan")}>
                        <span className="ms" aria-hidden="true">fast_forward</span>{t("blackhole.execute_full_plan")}
                    </button>
                </div>
                <div className="switch-guide-options">
                    <label className="switch-guide-toggle" title={t("custom_target.exclude_invalid_shapes_hint")}>
                        <input type="checkbox" checked={excludeInvalidShapes} disabled={executing}
                            onChange={event => {clearResult(); setExcludeInvalidShapes(event.target.checked);}}/>
                        <span className="switch-guide-toggle-track" aria-hidden="true"><span/></span>
                        <span>{t("custom_target.exclude_invalid_shapes")}</span>
                    </label>
                </div>
            </div>
            <details className="switch-guide-preferences">
                <summary><span className="ms" aria-hidden="true">bookmarks</span>{t("custom_target.preset_saved_list")}</summary>
                <form className={styles.presetToolbar} onSubmit={event => {event.preventDefault(); savePreset();}}>
                    <input className={`form-input ${styles.presetName}`} type="text" maxLength={80} value={presetName}
                        aria-label={t("custom_target.preset_name")} placeholder={t("custom_target.preset_name_placeholder")}
                        onChange={event => setPresetName(event.target.value)}/>
                    <button type="submit" className="switch-guide-button" disabled={!presetName.trim()}>
                        <span className="ms" aria-hidden="true">save</span>
                        {t(presets.some(preset => preset.name === presetName.trim()) ? "custom_target.preset_update" : "custom_target.preset_save")}
                    </button>
                    <button type="button" className="switch-guide-button" disabled={executing} onClick={() => {
                        setImportText(""); setImportError(""); setImportOpen(true);
                    }}>
                        <span className="ms" aria-hidden="true">content_paste</span>{t("custom_target.preset_import")}
                    </button>
                    <button type="button" className="switch-guide-button" onClick={exportPreset}>
                        <span className="ms" aria-hidden="true">content_copy</span>{t("custom_target.preset_export")}
                    </button>
                </form>
                <p className={styles.linkHint}>{t("custom_target.preset_hint")}</p>
                {presets.length > 0 && <ul className={styles.savedPresets} aria-label={t("custom_target.preset_saved_list")}>
                    {presets.map(preset => <li key={preset.name}>
                        <button type="button" className={styles.savedPreset} disabled={executing}
                            title={t("custom_target.preset_load", {name: preset.name})} onClick={() => loadPreset(preset)}>
                            <span className="ms" aria-hidden="true">bookmark</span><span>{preset.name}</span>
                        </button>
                        <button type="button" className={styles.iconButton} aria-label={t("custom_target.preset_remove", {name: preset.name})}
                            title={t("custom_target.preset_remove", {name: preset.name})} onClick={() => storePresets(presets.filter(item => item.name !== preset.name))}>
                            <span aria-hidden="true">×</span>
                        </button>
                    </li>)}
                </ul>}
            </details>
        </section>
        <div className={styles.editor}>
            <section className={`panel ${styles.board}`} aria-labelledby="custom-target-board">
                <div className={styles.sectionHeading}>
                    <div>
                        <div className={styles.boardTitle}>
                            <h2 id="custom-target-board">{t("custom_target.target_hand")}</h2>
                            <div className={styles.selectionToolbar}>
                                <button className={`switch-guide-button ${multiSelect ? "is-primary" : ""}`} aria-pressed={multiSelect} onClick={() => {
                                    setMultiSelect(!multiSelect);
                                    if (multiSelect) setSelected(indices => indices.slice(0, 1));
                                }}><span className="ms" aria-hidden="true">checklist</span>{t(multiSelect ? "custom_target.end_multi_select" : "custom_target.multi_select")}</button>
                                <button className="switch-guide-button" disabled={!groups.length} onClick={() => {
                                    setMultiSelect(true); setSelected(groups.map((_, index) => index));
                                }}>{t("custom_target.select_all")}</button>
                                {selected.length > 1 && <button type="button" className={styles.iconButton}
                                    aria-label={t("custom_target.remove_selected", {count: selected.length})}
                                    title={t("custom_target.remove_selected", {count: selected.length})} onClick={() => removeGroups(selected)}>
                                    <span className="ms" aria-hidden="true">delete</span>
                                </button>}
                            </div>
                        </div>
                        <p>{t("custom_target.target_hint")}</p>
                    </div>
                    <span className={`${styles.count} ${!valid ? styles.invalid : ""}`} aria-label={t("custom_target.count", {size, physical: size + quads, quads})}>
                        <strong>{size}</strong><span>/ 14</span>
                    </span>
                </div>
                <div className={styles.rack}>
                    <div className={styles.targets} role="group" aria-label={t("custom_target.target_hand")}>
                        {groups.map((group, index) => {
                            const face = group.rule.faces[0];
                            const kind = targetKind(group);
                            const label = `${groupLabel(group, index)}：${description(group)}`;
                            return <div key={index} className={styles.targetWrap}>
                                <button type="button" className={styles.target} aria-pressed={selected.includes(index)}
                                    aria-label={label} title={label} onClick={() => setSelected(indices => multiSelect ? toggle(indices, index) : [index])}>
                                    <span className={styles.targetTop}><span>{String(index + 1).padStart(2, "0")}</span>
                                        <span className={styles.targetLinks} aria-hidden="true">
                                            {(["suit_group", "distinct_group"] as const).map(key => group[key]?.trim() &&
                                                <span key={key} className={styles.linkDot} data-color={group[key]?.trim()} data-link={key}/>)}
                                        </span>
                                        {!selected.includes(index) && kind !== "single" && <b>×{kind === "quad" ? 4 : targetGroupSize(group)}</b>}
                                    </span>
                                    <div className={styles.targetFace} aria-hidden="true">
                                        {face ? <Tile tile={face} width={38} height={52}/> : <span className={styles.blankTile}>
                                            <span className="ms">{description(group) === t("custom_target.any") ? "all_inclusive" : "tune"}</span>
                                        </span>}
                                        {group.rule.faces.length > 1 && <small className={styles.faceCount}>+{group.rule.faces.length - 1}</small>}
                                    </div>
                                    <span className={styles.targetDescription}>{t(`custom_target.kind_${kind}`)} · {description(group)}</span>
                                </button>
                                <button type="button" className={styles.targetDelete}
                                    aria-label={t("custom_target.remove_slot", {index: index + 1})}
                                    title={t("custom_target.remove_slot", {index: index + 1})} onClick={() => removeGroups([index])}>
                                    <span aria-hidden="true">×</span>
                                </button>
                            </div>;
                        })}
                    </div>
                    <div className={styles.rackFooter}>
                        <button className="switch-guide-button" disabled={groups.length >= 14} onClick={() => {
                            changeGroups([...groups, newTargetGroup()]); setSelected([groups.length]);
                        }}><span className="ms" aria-hidden="true">add</span>{t("custom_target.add")}</button>
                        <button className={styles.textButton} onClick={() => {
                            changeGroups(Array.from({length: 14}, newTargetGroup)); setSelected([0]); setMultiSelect(false);
                        }}>
                            <span className="ms" aria-hidden="true">restart_alt</span>{t("custom_target.reset")}
                        </button>
                    </div>
                </div>
                <div className={styles.boardDetails}>
                    <div className={styles.countTrack} aria-hidden="true">{Array.from({length: 14}, (_, index) =>
                        <span key={index} data-filled={index < size} data-invalid={!valid}/>)}</div>
                    <p className={styles.countCaption}>{t("custom_target.count", {size, physical: size + quads, quads})}</p>
                    {active && <div className={styles.selection}>
                        <span className="ms" aria-hidden="true">edit</span>
                        <div><strong>{selected.length > 1 ? t("custom_target.conditions") : t("custom_target.edit_slot", {index: selected[0] + 1})}</strong>
                            <p>{description(active)}</p></div>
                    </div>}
                    <div className={styles.jokerStatus} aria-live="polite">
                        <div><span className="ms" aria-hidden="true">lock</span><strong>{t("custom_target.hand_joker")}</strong>
                            {currentState && <span className={styles.resultBadge}>{t(hasJoker ? "custom_target.joker_present" : "custom_target.joker_absent")}</span>}
                        </div>
                    </div>
                </div>
            </section>
            <section className={`panel ${styles.inspector}`} aria-label={t("custom_target.conditions")}>
                {active ? <div className={styles.filters}>
                    {selected.length > 1 && <button className={`switch-guide-button ${styles.batchApply}`} onClick={() =>
                        changeGroups(applyTargetRule(groups, selected, active.rule).map((group, index) => selected.includes(index)
                            ? {...group, suit_group: active.suit_group, distinct_group: active.distinct_group} : group))}>
                        <span className="ms" aria-hidden="true">content_copy</span>{t("custom_target.apply_conditions", {index: selected[0] + 1})}
                    </button>}
                    <div className={styles.segmented} role="group" aria-label={t("custom_target.group_type")}>
                        {TARGET_KINDS.map(kind => <button key={kind} type="button" aria-pressed={selected.every(index => targetKind(groups[index]) === kind)}
                            onClick={() => {if (selected.some(index => targetKind(groups[index]) !== kind)) changeKind(kind);}}>
                            {t(`custom_target.kind_${kind}`)}
                            <small>{t(kind === "quad" ? "custom_target.quad_count" : "custom_target.kind_count", {count: kind === "single" ? 1 : kind === "pair" ? 2 : 3})}</small>
                        </button>)}
                    </div>
                    <fieldset><legend>{t("custom_target.group_links")}</legend>
                        <div className={styles.linkFields}>
                            {(["suit_group", "distinct_group"] as const).map(key => <div key={key} role="group" aria-labelledby={`custom-target-${key}`}>
                                <span id={`custom-target-${key}`}>{t(key === "suit_group" ? "custom_target.suit_link" : "custom_target.distinct_link")}</span>
                                <div className={styles.chips}>
                                    {["", ...linkOptions(key)].map(value => {
                                        const pressed = selected.every(index => (groups[index][key]?.trim() ?? "") === value);
                                        const colorIndex = LINK_COLORS.indexOf(value);
                                        const name = value ? linkName(value) : t("custom_target.link_none");
                                        return <button key={value} type="button" aria-pressed={pressed}
                                            aria-label={name} title={name}
                                            onClick={() => {if (!pressed) changeLink(key, value);}}>
                                            {value && <span className={styles.linkDot} data-color={value} data-link={key} aria-hidden="true"/>}
                                            <span>{colorIndex >= 0 ? colorIndex + 1 : name}</span>
                                        </button>;
                                    })}
                                </div>
                            </div>)}
                        </div>
                    </fieldset>
                    <fieldset><legend>{t("custom_target.faces")}</legend>
                        <div className={styles.palette}>{SUITS.map(suit => <div className={styles.paletteRow} key={suit}>
                            <span>{t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)}</span>
                            <div>{Array.from({length: suit === "z" ? 7 : 10}, (_, i) => `${i < 9 || suit === "z" ? i + 1 : 0}${suit}`).map(face => {
                                const pressed = selected.every(index => groups[index].rule.faces.includes(face));
                                return <button type="button" key={face} aria-label={t("custom_target.select_face", {face})}
                                    aria-pressed={pressed} onClick={() => changeRule(rule => ({faces: toggle(rule.faces, face, !pressed)}))}>
                                    <Tile tile={face} width={27} height={37} shadow={false}/>
                                </button>;
                            })}</div>
                        </div>)}</div>
                    </fieldset>
                    <fieldset><legend>{t("custom_target.suits")}</legend><div className={styles.chips}>
                        {SUITS.map(suit => {
                            const pressed = selected.every(index => groups[index].rule.suits.includes(suit));
                            return <button type="button" key={suit} aria-pressed={pressed}
                                onClick={() => changeRule(rule => ({suits: toggle(rule.suits, suit, !pressed)}))}>{t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)}</button>;
                        })}
                    </div></fieldset>
                    <fieldset><legend>{t("custom_target.ranks")}</legend><div className={`${styles.chips} ${styles.ranks}`}>
                        {Array.from({length: 9}, (_, i) => i + 1).map(rank => {
                            const pressed = selected.every(index => groups[index].rule.ranks.includes(rank));
                            return <button type="button" key={rank} aria-pressed={pressed}
                                onClick={() => changeRule(rule => ({ranks: toggle(rule.ranks, rank, !pressed)}))}>{rank}</button>;
                        })}
                    </div></fieldset>
                    <fieldset><legend>{t("custom_target.properties")}</legend><div className={styles.chips}>
                        {(["red", "dora", "soul"] as const).map(key => {
                            const pressed = selected.every(index => groups[index].rule[key]);
                            return <button type="button" key={key} data-property={key} aria-pressed={pressed}
                                onClick={() => changeRule({[key]: !pressed})}><span className="ms" aria-hidden="true">{pressed ? "check_circle" : "add_circle_outline"}</span>{t(`custom_target.${key}`)}</button>;
                        })}
                    </div></fieldset>
                </div> : <div className={styles.empty}><span className="ms" aria-hidden="true">{groups.length ? "touch_app" : "add_box"}</span>
                    <p>{t(groups.length ? "custom_target.select_hint" : "custom_target.add_hint")}</p></div>}
            </section>
        </div>
        <section className={`panel ${styles.results}`} aria-live="polite">
            <div className={styles.sectionHeading}><div className={styles.heading}><span className="ms" aria-hidden="true">route</span><h2>{t("custom_target.result")}</h2></div>
                {data?.status === "plan" && <span className={styles.resultBadge}>{t("custom_target.steps")}</span>}
            </div>
            {searching && <div className={styles.empty}><span className={`ms ${styles.searchIcon}`} aria-hidden="true">manage_search</span>
                <strong>{t("advisor.searching")}</strong><p>{progress?.phase === "custom_searching" ? t("custom_target.progress", {nodes: progress.nodes, draws: progress.wall_prefix}) : t("custom_target.search_hint")}</p>
            </div>}
            {data?.status === "impossible" && <div className={styles.empty}><span className="ms" aria-hidden="true">search_off</span>
                <p>{t(`custom_target.reason_${reason}`, {defaultValue: t(`advisor.reason_${reason}`, {defaultValue: t("advisor.reason_unknown")})})}</p>
            </div>}
            {!data && <div className={styles.empty}><span className="ms" aria-hidden="true">conversion_path</span>
                <strong>{t("custom_target.empty")}</strong><p>{t(ready ? "custom_target.search_hint" : "blackhole.need_switch_stage")}</p>
            </div>}
            {stale && <p className={styles.notice} role="alert">{t("custom_target.stale")}</p>}
            {data?.status === "plan" && <>
                <div className={styles.planSummary}>
                    <span className="ms" aria-hidden="true">task_alt</span>
                    <strong>{t("custom_target.summary", {draws: data.draws_needed ?? 0, rounds: data.switch_discards?.length ?? 0})}</strong>
                </div>
                <div className={styles.resolvedHand}>{data.resolved_groups?.map((group, index) => <div className={styles.resolved} key={index}>
                    <span>{groupLabel(group, index)}</span>
                    <div className={styles.tiles}>{group.tiles.map(tile => <div key={tile.id}>
                        <Tile tile={tile.face} width={32} height={44}/>
                        {tile.joker_as && <small>{t("custom_target.joker_as", {face: tile.joker_as})}</small>}
                    </div>)}</div>
                </div>)}</div>
                <p className={styles.stepsHint}>{t("custom_target.steps_hint")}</p>
                {data.custom_steps?.length === 0 && <p className={styles.planSummary}>{t("custom_target.already_matches")}</p>}
                <ol className={styles.steps}>{data.custom_steps?.map((step, index) => <li key={index}>
                    <span className={styles.stepNumber} aria-hidden="true">{index + 1}</span>
                    <div className={styles.stepBody}><strong>{t(`custom_target.step_${step.kind}`)}</strong>
                        <div className={styles.stepTiles}>
                            {step.kind === "exchange" && <><div><span className={styles.stepLabel}>{t("advisor.switch_out")}</span>{tiles(step.out ?? [])}</div>
                                <span className={`ms ${styles.stepArrow}`} aria-hidden="true">arrow_forward</span></>}
                            <div>{step.kind === "exchange" && <span className={styles.stepLabel}>{t("advisor.switch_in")}</span>}{tiles(step.tiles)}</div>
                        </div>
                    </div>
                </li>)}</ol>
            </>}
        </section>
        <Modal open={importOpen} onClose={() => setImportOpen(false)} title={t("custom_target.preset_import")} width={640}>
            <form className={styles.importForm} onSubmit={event => {event.preventDefault(); importPreset();}}>
                <textarea className="form-input" rows={8} maxLength={1024 * 1024} value={importText}
                    aria-label={t("custom_target.preset_import_placeholder")} placeholder={t("custom_target.preset_import_placeholder")}
                    onChange={event => {setImportText(event.target.value); setImportError("");}}/>
                {importError && <p className={styles.notice} role="alert">{importError}</p>}
                <button type="submit" className="switch-guide-button" disabled={executing || !importText.trim()}>
                    {t("custom_target.preset_import")}
                </button>
            </form>
        </Modal>
    </div>;
}
