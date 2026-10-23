import React from "react";
import {useTranslation} from "react-i18next";
import Tile from "./Tile";
import type {GameStateData} from "../lib/gamestate";
import {newTargetGroup, targetSize, targetKind, targetGroupSize, TARGET_KINDS, applyTargetRule, type TargetGroup, type TargetRule, type TargetKind} from "../lib/customTarget";
import styles from "../pages/CustomSwitchPage.module.css";

const SUITS = ["m", "p", "s", "z"];
const LINK_COLORS = ["red", "orange", "yellow", "green", "cyan", "blue", "purple"];

export default function CustomTargetEditor({groups, onChange, currentState}: {
    groups: TargetGroup[];
    onChange: (groups: TargetGroup[]) => void;
    currentState?: GameStateData | null;
}) {
    const {t} = useTranslation();
    const [selected, setSelected] = React.useState<number[]>(() => groups.length ? [0] : []);
    const [multiSelect, setMultiSelect] = React.useState(false);
    const size = targetSize(groups);
    const quads = groups.filter(group => targetKind(group) === "quad").length;
    const valid = size === 14 && quads <= 4;
    const active = groups[selected[0]];
    const hasJoker = currentState?.hand_tiles.some(id => currentState.deck_map[String(id)] === "bd") ?? false;
    React.useEffect(() => {
        setSelected(indices => indices.filter(index => index < groups.length));
    }, [groups.length]);
    function changeGroups(next: TargetGroup[]) {
        onChange(next);
        setSelected(indices => indices.filter(index => index < next.length));
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
    return (
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
                                }}><span className="ms" aria-hidden="true">select_all</span>{t("custom_target.select_all")}</button>
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
                            <span className="ms" aria-hidden="true">{kind === "single" ? "crop_square" : kind === "pair" ? "filter_2" : kind === "quad" ? "filter_4" : "view_week"}</span>{t(`custom_target.kind_${kind}`)}
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
                                            {value ? <span className={styles.linkDot} data-color={value} data-link={key} aria-hidden="true"/> : <span className="ms" aria-hidden="true">link_off</span>}
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
                                onClick={() => changeRule(rule => ({suits: toggle(rule.suits, suit, !pressed)}))}><span className="ms" aria-hidden="true">style</span>{t(suit === "z" ? "about.tile_groups.honors" : `tile.suits.${suit}`)}</button>;
                        })}
                    </div></fieldset>
                    <fieldset><legend>{t("custom_target.ranks")}</legend><div className={`${styles.chips} ${styles.ranks}`}>
                        {Array.from({length: 9}, (_, i) => i + 1).map(rank => {
                            const pressed = selected.every(index => groups[index].rule.ranks.includes(rank));
                            return <button type="button" key={rank} aria-pressed={pressed}
                                onClick={() => changeRule(rule => ({ranks: toggle(rule.ranks, rank, !pressed)}))}><span className="ms" aria-hidden="true">tag</span>{rank}</button>;
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
    );
}
