import React from "react";
import {useTranslation} from "react-i18next";
import type {GameStateData} from "../lib/gamestate";
import styles from "./GameStatePage.module.css";
import {formatLevelIdToLabel} from "../lib/levelFormat";

type SummaryItem = {
    key: string;
    value: React.ReactNode;
};

type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

function valueOrDash(value: unknown) {
    if (value === null || value === undefined || value === "") return "-";
    return String(value);
}

function countOf(value: unknown[] | undefined) {
    return Array.isArray(value) ? value.length : 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
    return value !== null && typeof value === "object" && !Array.isArray(value);
}

function valueKind(value: unknown) {
    if (Array.isArray(value)) return `Array(${value.length})`;
    if (isRecord(value)) return `Object(${Object.keys(value).length})`;
    if (value === null) return "null";
    return typeof value;
}

function primitivePreview(value: unknown) {
    if (typeof value === "string") return `"${value}"`;
    if (value === undefined) return "undefined";
    return String(value);
}

function childPath(parent: string, childName: string) {
    return `${parent}.${childName.replace(/\\/g, "\\\\").replace(/\./g, "\\.")}`;
}

function GameStateTreeNode({
                               name,
                               value,
                               depth,
                               path,
                               expandedPaths,
                               onToggle,
                           }: {
    name: string;
    value: unknown;
    depth: number;
    path: string;
    expandedPaths: ReadonlySet<string>;
    onToggle: (path: string) => void;
}) {
    const {t} = useTranslation();
    const expandable = Array.isArray(value) || isRecord(value);
    const open = expandedPaths.has(path);

    if (!expandable) {
        return (
            <div className={styles.treeRow} style={{paddingLeft: depth * 16}}>
                <span className={styles.treeSpacer}/>
                <span className={styles.treeKey}>{name}</span>
                <span className={styles.treeColon}>:</span>
                <span className={styles.treePrimitive}>{primitivePreview(value)}</span>
                <span className={styles.treeType}>{valueKind(value)}</span>
            </div>
        );
    }

    const entries = Array.isArray(value)
        ? value.map((item, index) => [String(index), item] as const)
        : Object.entries(value);

    return (
        <div className={styles.treeNode}>
            <button
                type="button"
                className={styles.treeRowButton}
                style={{paddingLeft: depth * 16}}
                onClick={() => onToggle(path)}
                aria-expanded={open}
            >
                <span className={`ms ${styles.treeChevron}`} aria-hidden="true">
                    {open ? "expand_more" : "chevron_right"}
                </span>
                <span className={styles.treeKey}>{name}</span>
                <span className={styles.treeColon}>:</span>
                <span className={styles.treeType}>{valueKind(value)}</span>
            </button>
            {open ? (
                <div className={styles.treeChildren}>
                    {entries.length > 0 ? entries.map(([childName, childValue]) => (
                        <GameStateTreeNode
                            key={childName}
                            name={childName}
                            value={childValue}
                            depth={depth + 1}
                            path={childPath(path, childName)}
                            expandedPaths={expandedPaths}
                            onToggle={onToggle}
                        />
                    )) : (
                        <div className={styles.treeRow} style={{paddingLeft: (depth + 1) * 16}}>
                            <span className={styles.treeSpacer}/>
                            <span className={styles.treePrimitive}>{t("gamestate.tree_empty")}</span>
                        </div>
                    )}
                </div>
            ) : null}
        </div>
    );
}

export default function GameStatePage({currentState}: { currentState: GameStateData | null }) {
    const {t} = useTranslation();
    const [copied, setCopied] = React.useState(false);
    const [expandedPaths, setExpandedPaths] = React.useState<Set<string>>(() => new Set());

    const jsonText = React.useMemo(
        () => JSON.stringify(currentState ?? {}, null, 2),
        [currentState],
    );

    const summaryItems = React.useMemo<SummaryItem[]>(() => {
        if (!currentState) return [];
        return [
            {key: "stage", value: valueOrDash(currentState.stage)},
            {key: "level", value: formatLevelIdToLabel(currentState.level)},
            {key: "coin", value: valueOrDash(currentState.coin)},
            {key: "point", value: valueOrDash(currentState.point)},
            {key: "target_point", value: valueOrDash(currentState.target_point)},
            {key: "desktop_remain", value: valueOrDash(currentState.desktop_remain)},
            {key: "ended", value: currentState.stage === 100 ? t("gamestate.boolean_true") : t("gamestate.boolean_false")},
            {key: "hand_tiles", value: countOf(currentState.hand_tiles)},
            {key: "wall_tiles", value: countOf(currentState.wall_tiles)},
            {key: "locked_tiles", value: countOf(currentState.locked_tiles)},
            {key: "replacement_tiles", value: countOf(currentState.replacement_tiles)},
            {key: "effect_list", value: countOf(currentState.effect_list)},
            {key: "candidate_effect_list", value: countOf(currentState.candidate_effect_list)},
            {key: "goods", value: countOf(currentState.goods)},
            {key: "dora_tiles", value: countOf(currentState.dora_tiles)},
            {key: "update_reason", value: countOf(currentState.update_reason)},
        ];
    }, [currentState, t]);

    const copyJson = React.useCallback(async () => {
        try {
            await navigator.clipboard.writeText(jsonText);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 1400);
        } catch {
            setCopied(false);
        }
    }, [jsonText]);

    const toggleTreePath = React.useCallback((path: string) => {
        setExpandedPaths((previous) => {
            const next = new Set(previous);
            if (next.has(path)) next.delete(path);
            else next.add(path);
            return next;
        });
    }, []);

    return (
        <div className={styles.wrap}>
            <section className={`card ${styles.header}`}>
                <div>
                    <h2 className={styles.title}>{t("gamestate.title")}</h2>
                    <div className="hint">{t(currentState ? "gamestate.subtitle_live" : "gamestate.subtitle_empty")}</div>
                </div>
                <div className={styles.actions}>
                    <button className="nav-btn" onClick={copyJson} disabled={!currentState}>
                        {copied ? t("gamestate.copied") : t("gamestate.copy_json")}
                    </button>
                </div>
            </section>

            {currentState ? (
                <section className={`mj-panel card`}>
                    <h3 className={styles.sectionTitle}>{t("gamestate.summary_title")}</h3>
                    <div className={styles.summaryGrid}>
                        {summaryItems.map((item) => (
                            <div className={styles.summaryItem} key={item.key}>
                                <div className={styles.summaryLabel}>{item.key}</div>
                                <div className={styles.summaryValue}>{item.value}</div>
                            </div>
                        ))}
                    </div>
                </section>
            ) : null}

            <section className={`mj-panel card`}>
                <h3 className={styles.sectionTitle}>{t("gamestate.raw_title")}</h3>
                {currentState ? (
                    <div className={`selectable ${styles.tree}`}>
                        <GameStateTreeNode
                            name="game_state"
                            value={currentState as unknown as JsonValue}
                            depth={0}
                            path="game_state"
                            expandedPaths={expandedPaths}
                            onToggle={toggleTreePath}
                        />
                    </div>
                ) : (
                    <div className="empty">{t("gamestate.empty")}</div>
                )}
            </section>
        </div>
    );
}
