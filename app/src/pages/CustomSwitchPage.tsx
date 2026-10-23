import React from "react";
import {useTranslation} from "react-i18next";
import Tile from "../components/Tile";
import {compareTileFaces} from "../components/WallStats";
import Modal from "../components/Modal";
import CustomTargetEditor from "../components/CustomTargetEditor";
import type {GameStateData} from "../lib/gamestate";
import type {PlanData} from "../lib/planTypes";
import {targetSize, targetKind, restoreTargetGroups, parseTargetPreset, compactTargetPreset, readTargetPresets, TARGET_PRESETS_KEY, type TargetGroup, type TargetPreset} from "../lib/customTarget";
import {readCustomSwitchOptions, CUSTOM_SWITCH_TARGET_KEY, CUSTOM_SWITCH_WALL_LIMIT_KEY, CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY} from "../lib/customSwitchSettings";
import * as backendIpc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import styles from "./CustomSwitchPage.module.css";

export default function CustomSwitchPage({currentState, data, onClear}: {
    currentState: GameStateData | null;
    data: PlanData | null;
    onClear: () => void;
}) {
    const {t} = useTranslation();
    const [groups, setGroups] = React.useState(() => readCustomSwitchOptions().target_groups);
    const [editorVersion, setEditorVersion] = React.useState(0);
    const [wallLimit, setWallLimit] = React.useState(() => readCustomSwitchOptions().wall_limit);
    const [executing, setExecuting] = React.useState(false);
    const [excludeInvalidShapes, setExcludeInvalidShapes] = React.useState(() => readCustomSwitchOptions().exclude_invalid_shapes);
    const [presets, setPresets] = React.useState(readTargetPresets);
    const [presetName, setPresetName] = React.useState("");
    const [importOpen, setImportOpen] = React.useState(false);
    const [importText, setImportText] = React.useState("");
    const [importError, setImportError] = React.useState("");
    const searching = data?.status === "searching";
    const size = targetSize(groups);
    const quads = groups.filter(group => targetKind(group) === "quad").length;
    const ready = currentState && (currentState.source === "packet" ? [5, 6] : [4, 5]).includes(currentState.stage);
    const stale = data?.status === "plan" && data.state_revision !== currentState?.revision;
    const canExecute = ready && data?.status === "plan" && data.request_source === "live"
        && data.search_algorithm === "custom_target" && !!data.plan_id && !stale && !executing;
    const valid = size === 14 && quads <= 4;
    React.useEffect(() => {
        try { localStorage.setItem(CUSTOM_SWITCH_TARGET_KEY, JSON.stringify(groups)); } catch {}
    }, [groups]);
    React.useEffect(() => {
        try { localStorage.setItem(CUSTOM_SWITCH_WALL_LIMIT_KEY, String(wallLimit)); } catch {}
    }, [wallLimit]);
    React.useEffect(() => {
        try { localStorage.setItem(CUSTOM_SWITCH_EXCLUDE_INVALID_SHAPES_KEY, excludeInvalidShapes ? "1" : "0"); } catch {}
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
    }
    function currentPreset(): TargetPreset {
        return {format: "shanten-lens.custom-target", version: 1, name: presetName.trim() || t("custom_target.preset_default_name"),
            target_groups: groups, wall_limit: wallLimit, exclude_invalid_shapes: excludeInvalidShapes};
    }
    function storePresets(next: TargetPreset[]): boolean {
        try {
            localStorage.setItem(TARGET_PRESETS_KEY, JSON.stringify(next.map(compactTargetPreset)));
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
        setEditorVersion(value => value + 1);
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
    function groupLabel(group: Pick<TargetGroup, "quad" | "kind">, index: number) {
        return t("custom_target.kind_slot", {index: index + 1, kind: t(`custom_target.kind_${targetKind(group)}`)});
    }
    function tiles(ids: number[]) {
        const sorted = [...ids].sort((a, b) => compareTileFaces(currentState?.deck_map[String(a)] ?? "", currentState?.deck_map[String(b)] ?? ""));
        return <div className={styles.tiles}>{sorted.map(id => <span key={id} title={t("advisor.id_label", {id})}>
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
        <CustomTargetEditor key={editorVersion} groups={groups} onChange={changeGroups} currentState={currentState}/>
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
