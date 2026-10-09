import React from "react";
import {useTranslation} from "react-i18next";
import Tile from "../components/Tile";
import type {GameStateData} from "../lib/gamestate";
import type {PlanData} from "../lib/planTypes";
import {newTargetGroup, targetSize, restoreTargetGroups, applyTargetRule, type TargetGroup, type TargetRule} from "../lib/customTarget";
import * as backendIpc from "../lib/ipc";
import styles from "./CustomSwitchPage.module.css";

const STORAGE_KEY = "sl-custom-switch:target-v1";
const SUITS = ["m", "p", "s", "z"];

function readTarget(): TargetGroup[] {
    try {
        return restoreTargetGroups(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null"));
    } catch {}
    return Array.from({length: 14}, newTargetGroup);
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
    const searching = data?.status === "searching";
    const size = targetSize(groups);
    const quads = groups.filter(group => group.quad).length;
    const active = groups[selected[0]];
    const ready = currentState && [4, 5].includes(currentState.stage);
    const stale = data?.status === "plan" && data.state_revision !== currentState?.revision;
    const valid = size === 14 && quads <= 4;
    const hasJoker = currentState?.hand_tiles.some(id => currentState.deck_map[String(id)] === "bd") ?? false;
    React.useEffect(() => {
        try { localStorage.setItem(STORAGE_KEY, JSON.stringify(groups)); } catch {}
    }, [groups]);
    React.useEffect(() => {
        if (searching && !ready) void backendIpc.runSwitch({action: "stop", notify: false});
    }, [searching, ready]);

    function clearResult() {
        if (searching) void backendIpc.runSwitch({action: "stop", notify: false});
        onClear();
    }
    function changeGroups(next: TargetGroup[]) {
        clearResult();
        setGroups(next);
        setSelected(indices => indices.filter(index => index < next.length));
    }
    function changeRule(patch: Partial<TargetRule>) {
        if (active) changeGroups(applyTargetRule(groups, selected, {...active.rule, ...patch}));
    }
    function toggle<T>(values: T[], value: T): T[] {
        return values.includes(value) ? values.filter(item => item !== value) : [...values, value];
    }
    function description(rule: TargetRule) {
        return [rule.faces.join(" "), rule.suits.map(suit => t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)).join("/"),
            rule.ranks.join("/"), ...(["red", "dora", "soul"] as const).filter(key => rule[key]).map(key => t(`custom_target.${key}`))]
            .filter(Boolean).join(" · ") || t("custom_target.any");
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
                <label className={styles.wallLimit}>{t("blackhole.wall_limit")}
                    <input className="form-input" type="number" min={0} max={36} value={wallLimit}
                        onChange={event => {clearResult(); setWallLimit(Math.min(36, Math.max(0, Math.trunc(Number(event.target.value) || 0))));}}/>
                    <span>{t("custom_target.wall_hint")}</span>
                </label>
                {searching ? <button className="switch-guide-button is-danger" onClick={() => void backendIpc.runSwitch({action: "stop"})}>
                    <span className="ms" aria-hidden="true">stop_circle</span>{t("advisor.stop_search")}
                </button> : <button className="switch-guide-button is-primary" disabled={!ready || !valid} onClick={() => {
                    onClear();
                    void backendIpc.runSwitch({action: "start", options: {search_algorithm: "custom_target", target_groups: groups, wall_limit: wallLimit}});
                }}><span className="ms" aria-hidden="true">manage_search</span>{t("custom_target.start")}</button>}
            </div>
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
                            const label = `${t(group.quad ? "custom_target.quad_slot" : "custom_target.tile_slot", {index: index + 1})}：${description(group.rule)}`;
                            return <button key={index} type="button" className={styles.target} aria-pressed={selected.includes(index)}
                                aria-label={label} title={label} onClick={() => setSelected(indices => multiSelect ? toggle(indices, index) : [index])}>
                                <span className={styles.targetTop}><span>{String(index + 1).padStart(2, "0")}</span>
                                    {group.quad ? <b>×4</b> : selected.includes(index) && <span className="ms" aria-hidden="true">check</span>}
                                </span>
                                <div className={styles.targetFace} aria-hidden="true">
                                    {face ? <Tile tile={face} width={38} height={52}/> : <span className={styles.blankTile}>
                                        <span className="ms">{description(group.rule) === t("custom_target.any") ? "all_inclusive" : "tune"}</span>
                                    </span>}
                                    {group.rule.faces.length > 1 && <small className={styles.faceCount}>+{group.rule.faces.length - 1}</small>}
                                </div>
                                <span className={styles.targetDescription}>{description(group.rule)}</span>
                            </button>;
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
                    {!valid && <p className={styles.notice} role="status">{t("custom_target.invalid_count")}</p>}
                    {active && <div className={styles.selection}>
                        <span className="ms" aria-hidden="true">edit</span>
                        <div><strong>{selected.length > 1 ? t("custom_target.conditions") : t("custom_target.edit_slot", {index: selected[0] + 1})}</strong>
                            <p>{description(active.rule)}</p></div>
                    </div>}
                    <div className={styles.jokerStatus} aria-live="polite">
                        <div><span className="ms" aria-hidden="true">lock</span><strong>{t("custom_target.hand_joker")}</strong>
                            {currentState && <span className={styles.resultBadge}>{t(hasJoker ? "custom_target.joker_present" : "custom_target.joker_absent")}</span>}
                        </div>
                    </div>
                    <details className={styles.help}><summary>{t("custom_target.rules")}</summary>
                        <p>{t("custom_target.invalid_count")}</p><p>{t("custom_target.filters_hint")}</p><p>{t("custom_target.joker_hint")}</p>
                    </details>
                </div>
            </section>
            <section className={`panel ${styles.inspector}`} aria-labelledby="custom-target-inspector">
                <div className={styles.sectionHeading}>
                    <div className={styles.heading}><span className={styles.slotNumber}>{selected.length > 1 ? <span className="ms" aria-hidden="true">checklist</span> : active ? String(selected[0] + 1).padStart(2, "0") : "—"}</span>
                        <div><h2 id="custom-target-inspector">{t("custom_target.conditions")}</h2>
                            <p>{selected.length > 1 ? t("custom_target.batch_hint", {index: selected[0] + 1, count: selected.length}) : t("custom_target.conditions_hint")}</p></div>
                    </div>
                    {active && <button className={styles.iconButton} aria-label={t(selected.length > 1 ? "custom_target.remove_selected" : "custom_target.remove", {count: selected.length})}
                        title={t(selected.length > 1 ? "custom_target.remove_selected" : "custom_target.remove", {count: selected.length})} onClick={() => {
                            const next = groups.filter((_, index) => !selected.includes(index));
                            changeGroups(next); setSelected(next.length ? [Math.min(selected[0], next.length - 1)] : []);
                        }}><span className="ms" aria-hidden="true">delete</span></button>}
                </div>
                {active ? <div className={styles.filters}>
                    {selected.length > 1 && <button className={`switch-guide-button ${styles.batchApply}`} onClick={() => changeRule({})}>
                        <span className="ms" aria-hidden="true">content_copy</span>{t("custom_target.apply_conditions", {index: selected[0] + 1})}
                    </button>}
                    <div className={styles.segmented} role="group" aria-label={t("custom_target.group_type")}>
                        {[false, true].map(quad => <button key={String(quad)} type="button" aria-pressed={selected.every(index => groups[index].quad === quad)}
                            onClick={() => {if (selected.some(index => groups[index].quad !== quad)) changeGroups(groups.map((group, index) => selected.includes(index)
                                ? {...group, quad, rule: {...group.rule, joker: quad ? "exclude" : "allow"}} : group));}}>
                            {t(quad ? "custom_target.quad_short" : "custom_target.single")}
                            <small>{t(quad ? "custom_target.quad_count" : "custom_target.single_count")}</small>
                        </button>)}
                    </div>
                    <fieldset><legend>{t("custom_target.faces")}</legend>
                        <div className={styles.palette}>{SUITS.map(suit => <div className={styles.paletteRow} key={suit}>
                            <span>{t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)}</span>
                            <div>{Array.from({length: suit === "z" ? 7 : 10}, (_, i) => `${i < 9 || suit === "z" ? i + 1 : 0}${suit}`).map(face =>
                                <button type="button" key={face} aria-label={t("custom_target.select_face", {face})}
                                    aria-pressed={active.rule.faces.includes(face)} onClick={() => changeRule({faces: toggle(active.rule.faces, face)})}>
                                    <Tile tile={face} width={27} height={37} shadow={false}/>
                                </button>)}</div>
                        </div>)}</div>
                    </fieldset>
                    <fieldset><legend>{t("custom_target.suits")}</legend><div className={styles.chips}>
                        {SUITS.map(suit => <button type="button" key={suit} aria-pressed={active.rule.suits.includes(suit)}
                            onClick={() => changeRule({suits: toggle(active.rule.suits, suit)})}>{t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)}</button>)}
                    </div></fieldset>
                    <fieldset><legend>{t("custom_target.ranks")}</legend><div className={`${styles.chips} ${styles.ranks}`}>
                        {Array.from({length: 9}, (_, i) => i + 1).map(rank => <button type="button" key={rank} aria-pressed={active.rule.ranks.includes(rank)}
                            onClick={() => changeRule({ranks: toggle(active.rule.ranks, rank)})}>{rank}</button>)}
                    </div></fieldset>
                    <fieldset><legend>{t("custom_target.properties")}</legend><div className={styles.chips}>
                        {(["red", "dora", "soul"] as const).map(key => <button type="button" key={key} data-property={key} aria-pressed={active.rule[key]}
                            onClick={() => changeRule({[key]: !active.rule[key]})}><span className="ms" aria-hidden="true">{active.rule[key] ? "check_circle" : "add_circle_outline"}</span>{t(`custom_target.${key}`)}</button>)}
                    </div></fieldset>
                </div> : <div className={styles.empty}><span className="ms" aria-hidden="true">{groups.length ? "touch_app" : "add_box"}</span>
                    <p>{t(groups.length ? "custom_target.select_hint" : "custom_target.add_hint")}</p></div>}
            </section>
        </div>
        <section className={`panel ${styles.results}`} aria-live="polite">
            <div className={styles.sectionHeading}><div className={styles.heading}><span className="ms" aria-hidden="true">route</span><h2>{t("custom_target.result")}</h2></div>
                {data?.status === "plan" && <span className={styles.resultBadge}>{t("custom_target.manual")}</span>}
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
                    <span>{t(group.quad ? "custom_target.quad_slot" : "custom_target.tile_slot", {index: index + 1})}</span>
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
    </div>;
}
