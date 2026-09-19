import React from "react";
import {createPortal} from "react-dom";
import {useTranslation} from "react-i18next";
import Tile from "../components/Tile";
import characterCatalog from "../data/characters.json";
import fanCatalog from "../data/fan.json";
import * as backendIpc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import styles from "./GameRecordPage.module.css";

type ObjectValue = Record<string, unknown>;
type ActionItem = {index: number; source: "actions" | "records"; raw: ObjectValue; action: ObjectValue; name: string; data: ObjectValue | null};
type RoundGroup = {key: string; label: string; actions: ActionItem[]};
type TileView = {tile: string; sideways?: boolean; groupStart?: boolean};

function objectOf(value: unknown): ObjectValue | null {
    return value !== null && typeof value === "object" && !Array.isArray(value) ? value as ObjectValue : null;
}

function arrayOf(value: unknown): unknown[] {
    return Array.isArray(value) ? value : [];
}

function shortName(name: string) {
    return name.split(".").pop() || name;
}

function isTile(value: unknown): value is string {
    return typeof value === "string" && /^(?:bd|0[mps]|[1-9][mps]|[1-7]z)$/.test(value);
}

function tilesIn(value: unknown): string[] {
    if (Array.isArray(value)) return value.filter(isTile);
    if (typeof value !== "string") return [];
    return value.match(/(?:bd|0[mps]|[1-9][mps]|[1-7]z)/g) ?? [];
}

function sameTiles(left: readonly string[], right: readonly string[]) {
    const normalize = (tile: string) => tile.replace(/^0([mps])$/, "5$1");
    const sortedRight = right.map(normalize).sort();
    return left.length === right.length && left.map(normalize).sort().every((tile, index) => tile === sortedRight[index]);
}

function sameAddedGang(left: readonly string[], right: readonly string[]) {
    const normalize = (tile: string) => tile.replace(/^0([mps])$/, "5$1");
    return left.length === 3 && right.length === 4
        && [...left, ...right].map(normalize).every((tile, _, all) => tile === all[0]);
}

function meldTiles(tiles: string[], froms: unknown, seat: unknown): TileView[] {
    const sources = arrayOf(froms).map(Number);
    const owner = Number(seat);
    const calledIndex = sources.findIndex((source) => source !== owner);
    if (calledIndex < 0 || calledIndex >= tiles.length) return tiles.map((tile) => ({tile}));
    const source = sources[calledIndex];
    const called = tiles[calledIndex];
    const result = tiles.filter((_, index) => index !== calledIndex).map((tile) => ({tile} as TileView));
    const direction = (source - owner + 4) % 4;
    const target = direction === 3 ? 0 : direction === 2 ? 1 : result.length;
    result.splice(target, 0, {tile: called, sideways: true});
    return result;
}

function recordActions(record: ObjectValue | null): ActionItem[] {
    const wrapper = objectOf(record?.data);
    const detail = objectOf(wrapper?.data);
    const actions = arrayOf(detail?.actions);
    if (actions.length) {
        return actions.map((value, index) => {
            const action = objectOf(value) ?? {};
            const result = objectOf(action.result);
            return {
                index,
                source: "actions",
                raw: action,
                action,
                name: String(result?.name ?? ""),
                data: objectOf(result?.data),
            };
        });
    }
    return arrayOf(detail?.records).map((value, index) => {
        const result = objectOf(value);
        const raw = result ?? {};
        return {index, source: "records", raw, action: {result}, name: String(result?.name ?? ""), data: objectOf(result?.data)};
    });
}

function roundLabel(item: ActionItem, t: ReturnType<typeof useTranslation>["t"]) {
    const chang = Number(item.data?.chang ?? 0);
    const ju = Number(item.data?.ju ?? 0) + 1;
    const ben = Number(item.data?.ben ?? 0);
    return t("game_record.round_label", {
        wind: t(`game_record.wind.${chang}`, {defaultValue: String(chang)}),
        ju,
        ben,
    });
}

function groupActions(actions: ActionItem[], t: ReturnType<typeof useTranslation>["t"]): RoundGroup[] {
    const groups: RoundGroup[] = [];
    let current: RoundGroup = {key: "setup", label: t("game_record.setup"), actions: []};
    for (const action of actions) {
        if (shortName(action.name) === "RecordNewRound") {
            if (current.actions.length) groups.push(current);
            current = {key: `round-${action.index}`, label: roundLabel(action, t), actions: []};
        }
        current.actions.push(action);
    }
    if (current.actions.length) groups.push(current);
    return groups;
}

function actionIcon(name: string) {
    if (name === "RecordNewRound") return "flag";
    if (name === "RecordDealTile") return "add_circle";
    if (name === "RecordDiscardTile") return "output";
    if (name === "RecordChiPengGang" || name === "RecordAnGangAddGang") return "call_merge";
    if (name === "RecordBaBei") return "north";
    if (name.includes("Hule")) return "emoji_events";
    if (name === "RecordNoTile" || name === "RecordLiuJu") return "handshake";
    if (name === "RecordChangeTile") return "swap_horiz";
    return "more_horiz";
}

function actionTitle(item: ActionItem, t: ReturnType<typeof useTranslation>["t"]) {
    const name = shortName(item.name);
    if (!name) {
        if (item.action.userInput) return t("game_record.action.user_input");
        if (item.action.userEvent) return t("game_record.action.user_event");
        if (Number(item.action.gameEvent ?? 0)) return t("game_record.action.game_event");
        return t("game_record.action.protocol");
    }
    return t(`game_record.action.${name}`, {defaultValue: name});
}

function seatText(seat: unknown, players: ReadonlyMap<number, string>, t: ReturnType<typeof useTranslation>["t"]) {
    const value = Number(seat);
    if (!Number.isFinite(value)) return "";
    return t("game_record.seat", {seat: value, name: players.get(value) ?? "-"});
}

function operationText(type: unknown, t: ReturnType<typeof useTranslation>["t"]) {
    const key = ({
        1: "discard", 2: "chi", 3: "peng", 4: "an_gang", 5: "ming_gang", 6: "add_gang",
        7: "liqi", 8: "zimo", 9: "rong", 10: "liuju", 11: "babei",
    } as Record<number, string>)[Number(type)];
    return key
        ? t(`game_record.input.operation.${key}`)
        : t("game_record.input.operation.unknown", {type: type ?? "-"});
}

function protocolInputDescription(action: ObjectValue, players: ReadonlyMap<number, string>, t: ReturnType<typeof useTranslation>["t"]) {
    const input = objectOf(action.userInput);
    if (input) {
        const parts = [seatText(input.seat, players, t)];
        const operation = objectOf(input.operation);
        const cpg = objectOf(input.cpg);
        const vote = objectOf(input.vote);
        const choice = operation ?? cpg;
        if (choice) {
            parts.push(choice.cancelOperation === true
                ? t("game_record.input.skip", {operation: operationText(choice.type, t)})
                : operationText(choice.type, t));
            if (isTile(choice.tile)) parts.push(String(choice.tile));
            const changed = arrayOf(choice.changeTiles).filter(isTile);
            if (changed.length) parts.push(t("game_record.input.change_tiles", {tiles: changed.join(" ")}));
            if (choice.moqie === true) parts.push(t("game_record.moqie"));
            if (choice.autoOperation === true) parts.push(t("game_record.input.auto"));
            if (Object.prototype.hasOwnProperty.call(choice, "index")) parts.push(t("game_record.input.index", {index: choice.index}));
        } else if (vote) {
            parts.push(t(vote.yes === true ? "game_record.input.vote_yes" : "game_record.input.vote_no"));
        } else if (Number(input.emo) > 0) {
            parts.push(t("game_record.input.emoji", {id: input.emo}));
        } else {
            parts.push(t("game_record.input.type", {type: input.type ?? action.type ?? "-"}));
        }
        return parts.filter(Boolean).join(" · ");
    }
    const event = objectOf(action.userEvent);
    if (event) return [seatText(event.seat, players, t), t("game_record.input.user_event", {type: event.type ?? "-"})].filter(Boolean).join(" · ");
    if (Number(action.gameEvent ?? 0)) return t("game_record.input.game_event", {type: action.gameEvent});
    return t("game_record.protocol_action", {type: action.type ?? "-"});
}

function actionDescription(item: ActionItem, players: ReadonlyMap<number, string>, t: ReturnType<typeof useTranslation>["t"]) {
    const data = item.data;
    if (!data) {
        return protocolInputDescription(item.action, players, t);
    }
    const parts = [seatText(data.seat, players, t)].filter(Boolean);
    const name = shortName(item.name);
    if (name === "RecordDealTile") parts.push(t("game_record.left_tiles", {count: data.leftTileCount ?? "-"}));
    if (name === "RecordDiscardTile") {
        if (data.moqie === true) parts.push(t("game_record.moqie"));
        if (data.isLiqi === true || data.isWliqi === true) parts.push(t("game_record.liqi"));
    }
    if (name === "RecordChiPengGang" || name === "RecordAnGangAddGang") {
        const type = Number(data.type);
        const call = name === "RecordChiPengGang"
            ? ({0: "chi", 1: "peng", 2: "ming_gang"} as Record<number, string>)[type]
            : ({2: "add_gang", 3: "an_gang"} as Record<number, string>)[type];
        parts.push(call ? t(`game_record.call.${call}`) : t("game_record.call_type", {type: data.type ?? "-"}));
    }
    if (name.includes("Hule")) {
        const hules = arrayOf(data.hules).map(objectOf).filter((value): value is ObjectValue => Boolean(value));
        if (hules.length) parts.push(hules.map((hule) => seatText(hule.seat, players, t)).filter(Boolean).join(" / "));
    }
    if (name === "RecordNewRound") {
        parts.push(t("game_record.scores", {scores: arrayOf(data.scores).join(" / ")}));
    }
    return parts.join(" · ");
}

function findRecordedMeld(tiles: string[], seat: unknown, item: ActionItem, history: ActionItem[]) {
    return [...history].reverse().find((candidate) => {
        if (candidate.index >= item.index || Number(candidate.data?.seat) !== Number(seat)) return false;
        const name = shortName(candidate.name);
        const recordedTiles = tilesIn(candidate.data?.tiles);
        return name === "RecordChiPengGang" && (sameTiles(recordedTiles, tiles) || sameAddedGang(recordedTiles, tiles));
    });
}

function actionTiles(item: ActionItem, history: ActionItem[]): TileView[] {
    const data = item.data;
    if (!data) {
        const operation = objectOf(objectOf(item.action.userInput)?.operation);
        return [operation?.tile, ...arrayOf(operation?.changeTiles)].filter(isTile).map((tile) => ({tile})).slice(0, 18);
    }
    const name = shortName(item.name);
    const values: TileView[] = [];
    const pushGroup = (tiles: TileView[]) => values.push(...tiles.map((tile, index) => ({...tile, groupStart: values.length > 0 && index === 0})));
    if (name === "RecordNewRound") pushGroup(tilesIn(data.doras).map((tile) => ({tile})));
    else if (name.includes("Hule")) {
        for (const hule of arrayOf(data.hules).map(objectOf)) {
            if (!hule) continue;
            pushGroup([...tilesIn(hule.hand), ...tilesIn(hule.huTile)].map((tile) => ({tile})));
            for (const rawMeld of arrayOf(hule.ming)) {
                const tiles = tilesIn(rawMeld);
                const recorded = findRecordedMeld(tiles, hule.seat, item, history);
                pushGroup(recorded ? meldTiles(tiles, recorded.data?.froms, hule.seat) : tiles.map((tile) => ({tile})));
            }
        }
    } else if (name === "RecordChiPengGang") {
        pushGroup(meldTiles(tilesIn(data.tiles), data.froms, data.seat));
    } else if (name === "RecordAnGangAddGang") {
        pushGroup(tilesIn(data.tiles).map((tile) => ({tile})));
    } else {
        pushGroup([...tilesIn(data.tile), ...tilesIn(data.tiles)].map((tile) => ({tile})));
    }
    return values.slice(0, 30);
}

function JsonNode({label, value, root = false}: {label: string; value: unknown; root?: boolean}) {
    const container = Array.isArray(value) || objectOf(value) !== null;
    const entries = Array.isArray(value)
        ? value.map((child, index) => [String(index), child] as const)
        : Object.entries(objectOf(value) ?? {});
    const [open, setOpen] = React.useState(root);
    if (!container) {
        return (
            <div className={styles.jsonRow}>
                <span className={styles.jsonSpacer}/><span className={styles.jsonKey}>{label}:</span>
                <span className={styles.jsonPrimitive} data-type={value === null ? "null" : typeof value}>{JSON.stringify(value) ?? "undefined"}</span>
            </div>
        );
    }
    const type = Array.isArray(value) ? `[${entries.length}]` : `{${entries.length}}`;
    return (
        <div className={styles.jsonNode}>
            <button className={styles.jsonRow} type="button" onClick={() => setOpen((current) => !current)} aria-expanded={open}>
                <span className={`ms ${styles.jsonChevron}`} data-open={open}>chevron_right</span>
                <span className={styles.jsonKey}>{label}</span><span className={styles.jsonType}>{type}</span>
            </button>
            {open ? <div className={styles.jsonChildren}>{entries.map(([key, child]) => <JsonNode key={key} label={key} value={child}/>)}</div> : null}
        </div>
    );
}

type FieldKind = "number" | "string" | "boolean" | "tile" | "gangTile" | "tiles" | "wall" | "tileGroups" | "fans" | "numbers" | "callType" | "gangType" | "titleId" | "wind";
type FieldSpec = {key: string; kind: FieldKind};
type MeldKind = "chi" | "peng" | "an_gang" | "ming_gang" | "add_gang";

const MELD_PREFIX: Record<MeldKind, string> = {
    chi: "shunzi", peng: "kezi", an_gang: "angang", ming_gang: "gangzi", add_gang: "jiagang",
};

function meldKindOf(value: unknown): MeldKind {
    const text = String(value ?? "");
    const matched = (Object.entries(MELD_PREFIX) as [MeldKind, string][]).find(([, prefix]) => text.startsWith(`${prefix}(`));
    if (matched) return matched[0];
    const tiles = tilesIn(value);
    if (tiles.length === 4) return "ming_gang";
    return tiles.length === 3 && new Set(tiles.map((tile) => tile.replace(/^0/, "5"))).size === 3 ? "chi" : "peng";
}

function encodeMeld(kind: MeldKind, tiles: string[]) {
    return `${MELD_PREFIX[kind]}(${tiles.join(",")})`;
}

function gangTiles(tile: string) {
    const suit = tile.match(/^[05]([mps])$/)?.[1];
    return suit ? [`0${suit}`, `5${suit}`, `5${suit}`, `5${suit}`] : Array(4).fill(tile);
}

const TILE_PALETTE_GROUPS = [
    ...["m", "p", "s"].map((suit) => ({key: suit, tiles: ["1", "2", "3", "4", "5", "0", "6", "7", "8", "9"].map((value) => `${value}${suit}`)})),
    {key: "z", tiles: ["1z", "2z", "3z", "4z", "5z", "6z", "7z", "bd"]},
];
const SKIN_CATALOG = characterCatalog.flatMap((character) => character.skins);

const EVENT_FIELDS: Record<string, FieldSpec[]> = {
    RecordNewRound: [
        {key: "chang", kind: "wind"}, {key: "ju", kind: "number"}, {key: "ben", kind: "number"},
        {key: "doras", kind: "tiles"}, {key: "scores", kind: "numbers"}, {key: "liqibang", kind: "number"},
        {key: "tiles0", kind: "tiles"}, {key: "tiles1", kind: "tiles"}, {key: "tiles2", kind: "tiles"}, {key: "tiles3", kind: "tiles"},
        {key: "paishan", kind: "wall"}, {key: "leftTileCount", kind: "number"},
    ],
    RecordDealTile: [
        {key: "seat", kind: "number"}, {key: "tile", kind: "tile"}, {key: "leftTileCount", kind: "number"},
    ],
    RecordDiscardTile: [
        {key: "seat", kind: "number"}, {key: "tile", kind: "tile"}, {key: "isLiqi", kind: "boolean"},
        {key: "moqie", kind: "boolean"}, {key: "isWliqi", kind: "boolean"},
    ],
    RecordChiPengGang: [
        {key: "seat", kind: "number"}, {key: "type", kind: "callType"}, {key: "tiles", kind: "tiles"},
        {key: "froms", kind: "numbers"}, {key: "scores", kind: "numbers"}, {key: "liqibang", kind: "number"},
    ],
    RecordAnGangAddGang: [
        {key: "seat", kind: "number"}, {key: "type", kind: "gangType"}, {key: "tiles", kind: "gangTile"},
    ],
    RecordBaBei: [
        {key: "seat", kind: "number"}, {key: "moqie", kind: "boolean"},
    ],
    RecordNoTile: [{key: "liujumanguan", kind: "boolean"}, {key: "gameend", kind: "boolean"}],
    RecordHule: [
        {key: "oldScores", kind: "numbers"}, {key: "deltaScores", kind: "numbers"}, {key: "scores", kind: "numbers"},
        {key: "doras", kind: "tiles"}, {key: "baopai", kind: "number"},
    ],
    RecordHuleXueZhanMid: [
        {key: "oldScores", kind: "numbers"}, {key: "deltaScores", kind: "numbers"}, {key: "scores", kind: "numbers"}, {key: "doras", kind: "tiles"},
    ],
    RecordHuleXueZhanEnd: [
        {key: "oldScores", kind: "numbers"}, {key: "deltaScores", kind: "numbers"}, {key: "scores", kind: "numbers"},
        {key: "doras", kind: "tiles"},
    ],
};

const HULE_FIELDS: FieldSpec[] = [
    {key: "seat", kind: "number"}, {key: "hand", kind: "tiles"}, {key: "ming", kind: "tileGroups"}, {key: "huTile", kind: "tile"},
    {key: "zimo", kind: "boolean"}, {key: "qinjia", kind: "boolean"}, {key: "liqi", kind: "boolean"},
    {key: "doras", kind: "tiles"}, {key: "liDoras", kind: "tiles"}, {key: "yiman", kind: "boolean"},
    {key: "titleId", kind: "titleId"}, {key: "fans", kind: "fans"}, {key: "count", kind: "number"}, {key: "fu", kind: "number"}, {key: "pointSum", kind: "number"}, {key: "dadian", kind: "number"},
];

const DEFAULT_HULE: ObjectValue = {
    hand: [], ming: [], huTile: "", seat: 0,
    zimo: false, qinjia: false, liqi: false, doras: [], liDoras: [], yiman: false,
    count: 0, fans: [], fu: 0, title: "", titleId: 0,
    pointRong: 0, pointZimoQin: 0, pointZimoXian: 0, pointSum: 0, dadian: 0,
    baopai: 0, baopaiSeats: [], lines: [], tianmingBonus: 0, baidaChanged: [],
    huTileBaiDaChanged: "", xiaKeShangCoefficient: 0,
};

const DEFAULT_HULE_DATA: ObjectValue = {
    hules: [DEFAULT_HULE], oldScores: [], deltaScores: [], scores: [],
    gameend: {scores: []}, doras: [], baopai: 0,
};

const XUEZHAN_HULE_FIELDS: FieldSpec[] = [
    {key: "seat", kind: "number"}, {key: "hand", kind: "tiles"}, {key: "ming", kind: "tileGroups"}, {key: "huTile", kind: "tile"},
    {key: "zimo", kind: "boolean"}, {key: "yiman", kind: "boolean"}, {key: "titleId", kind: "titleId"}, {key: "fans", kind: "fans"}, {key: "count", kind: "number"}, {key: "fu", kind: "number"},
];

function replacePath(value: ObjectValue, path: (string | number)[], nextValue: unknown): ObjectValue {
    const next = structuredClone(value);
    let target: any = next;
    for (let index = 0; index < path.length - 1; index += 1) {
        const key = path[index];
        if (target[key] === null || typeof target[key] !== "object") target[key] = typeof path[index + 1] === "number" ? [] : {};
        target = target[key];
    }
    target[path[path.length - 1]] = nextValue;
    return next;
}

function withEventDefaults(value: ObjectValue): ObjectValue {
    const wrapperPath = objectOf(value.result) ? ["result"] : [];
    const wrapper = objectOf(value.result) ?? value;
    if (shortName(String(wrapper.name ?? "")) !== "RecordHule") return value;
    const data = objectOf(wrapper.data) ?? {};
    const hules = arrayOf(data.hules);
    return replacePath(value, [...wrapperPath, "data"], {
        ...DEFAULT_HULE_DATA,
        ...data,
        hules: hules.length ? hules.map((hule) => ({...DEFAULT_HULE, ...(objectOf(hule) ?? {})})) : [DEFAULT_HULE],
    });
}

function TilePicker({tiles, single = false, onChange}: {tiles: string[]; single?: boolean; onChange: (tiles: string[]) => void}) {
    const {t} = useTranslation();
    return (
        <div className={styles.tilePicker}>
            <div className={styles.collectionHeader}>
                <strong>{t("game_record.editor.selected_tiles")}</strong>
                <span>{t("game_record.editor.item_count", {count: tiles.length})}</span>
            </div>
            <div className={styles.selectedTiles}>
                {tiles.map((tile, index) => (
                    <button key={`${tile}-${index}`} type="button" onClick={() => onChange(single ? [] : tiles.filter((_, tileIndex) => tileIndex !== index))} aria-label={t("game_record.editor.remove_tile", {tile})} title={t("game_record.editor.remove_tile", {tile})}>
                        <Tile tile={tile} width={34} height={45} shadow={false}/><span className="ms" aria-hidden="true">close</span>
                    </button>
                ))}
                {!tiles.length ? <small>{t("game_record.editor.no_tiles")}</small> : null}
            </div>
            <details className={styles.tilePalette}>
                <summary><span className="ms" aria-hidden="true">grid_view</span>{t("game_record.editor.tile_palette")}</summary>
                <div className={styles.tilePaletteBody}>
                    {TILE_PALETTE_GROUPS.map((group) => (
                        <section key={group.key} className={styles.tilePaletteGroup}>
                            <span>{t(`game_record.editor.tile_suit.${group.key}`)}</span>
                            <div>{group.tiles.map((tile) => (
                                <button key={tile} type="button" onClick={() => onChange(single ? [tile] : [...tiles, tile])} aria-label={t("game_record.editor.add_tile_named", {tile})} title={t("game_record.editor.add_tile_named", {tile})}>
                                    <Tile tile={tile} width={34} height={45} shadow={false}/>
                                </button>
                            ))}</div>
                        </section>
                    ))}
                </div>
            </details>
        </div>
    );
}

function FieldInput({spec, value, onChange}: {spec: FieldSpec; value: unknown; onChange: (value: unknown) => void}) {
    const {t, i18n} = useTranslation();
    const label = t(`game_record.editor.field.${spec.key}`, {defaultValue: spec.key});
    if (spec.kind === "boolean") {
        return <label className={styles.booleanField}><input type="checkbox" checked={value === true} onChange={(event) => onChange(event.target.checked)}/><span>{label}</span></label>;
    }
    if (spec.kind === "callType") {
        return (
            <label className={styles.formField}>
                <span>{label}</span>
                <select value={Number(value)} onChange={(event) => onChange(Number(event.target.value))}>
                    <option value={0}>{t("game_record.call.chi")}</option>
                    <option value={1}>{t("game_record.call.peng")}</option>
                    <option value={2}>{t("game_record.call.ming_gang")}</option>
                </select>
            </label>
        );
    }
    if (spec.kind === "gangType") {
        return (
            <label className={styles.formField}>
                <span>{label}</span>
                <select value={Number(value ?? 2)} onChange={(event) => onChange(Number(event.target.value))}>
                    <option value={2}>{t("game_record.call.add_gang")}</option>
                    <option value={3}>{t("game_record.call.an_gang")}</option>
                </select>
            </label>
        );
    }
    if (spec.kind === "wind") {
        return (
            <label className={styles.formField}>
                <span>{label}</span>
                <select value={Number(value ?? 0)} onChange={(event) => onChange(Number(event.target.value))}>
                    {[0, 1, 2, 3].map((wind) => <option key={wind} value={wind}>{wind} · {t(`game_record.wind.${wind}`)}</option>)}
                </select>
            </label>
        );
    }
    if (spec.kind === "titleId") {
        return (
            <label className={styles.formField}>
                <span>{label}</span>
                <select value={Number(value ?? 0)} onChange={(event) => onChange(Number(event.target.value))}>
                    {Array.from({length: 12}, (_, id) => <option key={id} value={id}>{id} · {t(`game_record.editor.title_id_options.${id}`)}</option>)}
                </select>
            </label>
        );
    }
    if (spec.kind === "tile" || spec.kind === "gangTile" || spec.kind === "tiles" || spec.kind === "wall") {
        const valueTiles = tilesIn(value);
        const tiles = spec.kind === "gangTile" && valueTiles[0] ? gangTiles(valueTiles[0]) : valueTiles;
        return (
            <div className={`${styles.formField} ${styles.tileField}`}>
                <span>{label}</span>
                <TilePicker tiles={tiles} single={spec.kind === "tile" || spec.kind === "gangTile"} onChange={(next) => onChange(spec.kind === "wall" ? next.join("") : spec.kind === "tile" || spec.kind === "gangTile" ? next[0] ?? "" : next)}/>
            </div>
        );
    }
    if (spec.kind === "tileGroups") {
        const groups = arrayOf(value).map((raw) => ({raw, tiles: tilesIn(raw), kind: meldKindOf(raw)}));
        return (
            <fieldset className={`${styles.editorGroup} ${styles.tileGroups}`}>
                <legend>{label}</legend>
                {groups.map((group, index) => (
                    <div key={index} className={styles.tileGroupEditor}>
                        <span>{t("game_record.editor.meld", {index: index + 1})}</span>
                        <label className={styles.formField}><span>{t("game_record.editor.meld_type")}</span><select value={group.kind} onChange={(event) => onChange(groups.map((current, groupIndex) => groupIndex === index ? encodeMeld(event.target.value as MeldKind, current.tiles) : current.raw))}>
                            {(["chi", "peng", "an_gang", "ming_gang", "add_gang"] as MeldKind[]).map((kind) => <option key={kind} value={kind}>{t(`game_record.call.${kind}`)}</option>)}
                        </select></label>
                        <TilePicker tiles={group.tiles} onChange={(next) => onChange(groups.map((current, groupIndex) => groupIndex === index ? encodeMeld(current.kind, next) : current.raw))}/>
                        <button type="button" onClick={() => onChange(groups.filter((_, groupIndex) => groupIndex !== index).map((current) => current.raw))}>{t("game_record.editor.remove_meld")}</button>
                    </div>
                ))}
                <button type="button" onClick={() => onChange([...groups.map((group) => group.raw), encodeMeld("chi", [])])}>{t("game_record.editor.add_meld")}</button>
            </fieldset>
        );
    }
    if (spec.kind === "fans") {
        const fans = arrayOf(value).map((fan) => objectOf(fan) ?? {});
        const language = (i18n.resolvedLanguage || i18n.language).startsWith("ja") ? "jp" : "chs";
        const updateFan = (index: number, key: "id" | "val", next: string) => onChange(fans.map((fan, fanIndex) => fanIndex === index
            ? {...fan, [key]: Number(next), ...(key === "id" ? {name: ""} : {})}
            : fan));
        return (
            <fieldset className={`${styles.editorGroup} ${styles.fanEditor}`}>
                <legend>{label}</legend>
                {fans.map((fan, index) => (
                    <div key={index} className={styles.fanRow}>
                        <label className={styles.formField}><span>{t("game_record.editor.fan_name")}</span><select value={Number(fan.id ?? 0)} onChange={(event) => updateFan(index, "id", event.target.value)}>
                            {!fanCatalog.some((option) => option.id === Number(fan.id)) ? <option value={Number(fan.id ?? 0)}>{t("game_record.editor.unknown_fan", {id: fan.id ?? 0})}</option> : null}
                            {fanCatalog.map((option) => <option key={option.id} value={option.id}>{option.name[language]} ({option.id})</option>)}
                        </select></label>
                        <label className={styles.formField}><span>{t("game_record.editor.fan_value")}</span><input type="number" min={0} value={Number(fan.val ?? 0)} onChange={(event) => updateFan(index, "val", event.target.value)}/></label>
                        <button className={`${styles.iconEditorButton} ${styles.borderlessDeleteButton}`} type="button" aria-label={t("game_record.editor.remove_fan")} title={t("game_record.editor.remove_fan")} onClick={() => onChange(fans.filter((_, fanIndex) => fanIndex !== index))}><span className="ms" aria-hidden="true">delete</span></button>
                    </div>
                ))}
                <button type="button" onClick={() => onChange([...fans, {id: 0, name: "", val: 1}])}>{t("game_record.editor.add_fan")}</button>
            </fieldset>
        );
    }
    if (spec.kind === "numbers") {
        const numbers = arrayOf(value).map(Number);
        return (
            <fieldset className={`${styles.editorGroup} ${styles.numberArray}`}>
                <legend>{label}</legend>
                <div className={styles.collectionHeader}><span>{t("game_record.editor.number_array")}</span><span>{t("game_record.editor.item_count", {count: numbers.length})}</span></div>
                <div className={styles.numberArrayItems}>
                    {numbers.map((number, index) => (
                        <div key={index} className={styles.numberArrayItem}>
                            <span>#{index}</span>
                            <input type="number" value={number} aria-label={t("game_record.editor.array_item", {index: index + 1})} onChange={(event) => onChange(numbers.map((current, itemIndex) => itemIndex === index ? Number(event.target.value) : current))}/>
                            <button className={`${styles.iconEditorButton} ${styles.borderlessDeleteButton}`} type="button" aria-label={t("game_record.editor.remove_array_item", {index: index + 1})} title={t("game_record.editor.remove_array_item", {index: index + 1})} onClick={() => onChange(numbers.filter((_, itemIndex) => itemIndex !== index))}><span className="ms" aria-hidden="true">delete</span></button>
                        </div>
                    ))}
                    <button className={`${styles.addArrayItem} ${styles.iconEditorButton}`} type="button" aria-label={t("game_record.editor.add_array_item")} title={t("game_record.editor.add_array_item")} onClick={() => onChange([...numbers, 0])}><span className="ms" aria-hidden="true">add</span></button>
                </div>
            </fieldset>
        );
    }
    const shown = String(value ?? "");
    const accept = (text: string) => {
        if (spec.kind === "number") onChange(Number(text));
        else onChange(text);
    };
    return <label className={styles.formField}><span>{label}</span><input type={spec.kind === "number" ? "number" : "text"} value={shown} onChange={(event) => accept(event.target.value)}/></label>;
}

function EditorDialog({title, children, onClose, bodyClassName = ""}: {title: string; children: React.ReactNode; onClose: () => void; bodyClassName?: string}) {
    const {t} = useTranslation();
    const dialogRef = React.useRef<HTMLElement | null>(null);
    React.useEffect(() => {
        const closeOnEscape = (event: KeyboardEvent) => {
            const dialogs = document.querySelectorAll<HTMLElement>("[role=dialog]");
            if (event.key === "Escape" && dialogs[dialogs.length - 1] === dialogRef.current) onClose();
        };
        window.addEventListener("keydown", closeOnEscape);
        return () => window.removeEventListener("keydown", closeOnEscape);
    }, [onClose]);
    return createPortal(
        <div className={styles.editorBackdrop} onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
            <section ref={dialogRef} className={styles.editorDialog} role="dialog" aria-modal="true" aria-label={title}>
                <header><h3>{title}</h3><button type="button" onClick={onClose} aria-label={t("game_record.editor.close")}><span className="ms" aria-hidden="true">close</span></button></header>
                <div className={`${styles.editorDialogBody} ${bodyClassName}`}>{children}</div>
            </section>
        </div>,
        document.body,
    );
}

function SkinPicker({characterId, skinId, onSelect, onClose}: {characterId: number; skinId: number; onSelect: (characterId: number, skinId: number) => void; onClose: () => void}) {
    const {t, i18n} = useTranslation();
    const language = (i18n.resolvedLanguage || i18n.language).startsWith("ja") ? "jp" : "chs";
    const initialCharacter = characterCatalog.find((character) => character.id === characterId) ?? characterCatalog[0];
    const [selectedCharacterId, setSelectedCharacterId] = React.useState(initialCharacter.id);
    const initialSkins = initialCharacter.skins;
    const [selectedSkinId, setSelectedSkinId] = React.useState(initialSkins.some((skin) => skin.id === skinId) ? skinId : initialSkins[0]?.id ?? 0);
    const selectedCharacter = characterCatalog.find((character) => character.id === selectedCharacterId) ?? initialCharacter;
    const selectCharacter = (nextCharacterId: number) => {
        const character = characterCatalog.find((item) => item.id === nextCharacterId);
        setSelectedCharacterId(nextCharacterId);
        setSelectedSkinId(character?.skins[0]?.id ?? 0);
    };
    return (
        <EditorDialog title={t("game_record.editor.skin_picker")} onClose={onClose} bodyClassName={styles.skinDialogBody}>
            <div className={styles.skinPicker}>
                <div className={styles.skinPickerColumns}>
                    <section>
                        <h4>{t("game_record.editor.character")}</h4>
                        <div className={styles.skinPickerList} role="listbox" aria-label={t("game_record.editor.character")}>
                            {characterCatalog.map((character) => (
                                <button key={character.id} className={character.id === selectedCharacterId ? styles.pickerSelected : ""} type="button" role="option" aria-selected={character.id === selectedCharacterId} autoFocus={character.id === selectedCharacterId} onClick={() => selectCharacter(character.id)}>
                                    <span>{character.name[language] || character.name.chs}</span><small>{character.id}</small>
                                </button>
                            ))}
                        </div>
                    </section>
                    <section>
                        <h4>{t("game_record.editor.skin")}</h4>
                        <div className={styles.skinPickerList} role="listbox" aria-label={t("game_record.editor.skin")}>
                            {selectedCharacter.skins.map((skin) => (
                                <button key={skin.id} className={skin.id === selectedSkinId ? styles.pickerSelected : ""} type="button" role="option" aria-selected={skin.id === selectedSkinId} onClick={() => setSelectedSkinId(skin.id)}>
                                    <span>{skin.name[language] || skin.name.chs}</span><small>{skin.id}</small>
                                </button>
                            ))}
                        </div>
                    </section>
                </div>
                <div className={`${styles.editorButtons} ${styles.skinPickerFooter}`}>
                    <button type="button" onClick={onClose}>{t("game_record.editor.cancel")}</button>
                    <button type="button" disabled={!selectedSkinId} onClick={() => onSelect(selectedCharacterId, selectedSkinId)}>{t("game_record.editor.select_skin")}</button>
                </div>
            </div>
        </EditorDialog>
    );
}

function SpecializedEditor({kind, value, onChange}: {kind: "head" | "event"; value: ObjectValue; onChange: (value: ObjectValue) => void}) {
    const {t, i18n} = useTranslation();
    const language = (i18n.resolvedLanguage || i18n.language).startsWith("ja") ? "jp" : "chs";
    const [skinPlayer, setSkinPlayer] = React.useState<number | null>(null);
    if (kind === "head") {
        const accounts = arrayOf(value.accounts).map(objectOf);
        return (
            <div className={styles.specialEditor}>
                {accounts.map((account, index) => account ? (
                    <fieldset key={index} className={`${styles.editorGroup} ${styles.playerEditor}`}>
                        <legend>{t("game_record.editor.player", {seat: account.seat ?? index})}</legend>
                        <div className={`${styles.formGrid} ${styles.playerAccountGrid}`}>
                            {(["accountId", "seat", "nickname", "avatarId", "avatarFrame"] as const).map((key) => (
                                <FieldInput key={key} spec={{key, kind: key === "nickname" ? "string" : "number"}} value={account[key]} onChange={(next) => onChange(replacePath(value, ["accounts", index, key], next))}/>
                            ))}
                        </div>
                        <section className={styles.characterEditor}>
                            <header>
                                <strong>{t("game_record.editor.character")}</strong>
                                <button className={styles.changeSkinButton} type="button" onClick={() => setSkinPlayer(index)}>{t("game_record.editor.change_skin")}</button>
                            </header>
                            <div className={`${styles.formGrid} ${styles.playerCharacterGrid}`}>
                                {(["charid", "skin", "isUpgraded", "level"] as const).map((key) => {
                                    const fieldValue = objectOf(account.character)?.[key];
                                    const item = key === "charid"
                                        ? characterCatalog.find((character) => character.id === Number(fieldValue))
                                        : key === "skin" ? SKIN_CATALOG.find((skin) => skin.id === Number(fieldValue)) : undefined;
                                    const field = <FieldInput spec={{key, kind: key === "isUpgraded" ? "boolean" : "number"}} value={fieldValue} onChange={(next) => onChange(replacePath(value, ["accounts", index, "character", key], next))}/>;
                                    return item ? (
                                        <div key={key} className={styles.namedIdField}>{field}<span title={item.name[language] || item.name.chs}>{item.name[language] || item.name.chs}</span></div>
                                    ) : <React.Fragment key={key}>{field}</React.Fragment>;
                                })}
                            </div>
                        </section>
                    </fieldset>
                ) : null)}
                {skinPlayer !== null && accounts[skinPlayer] ? (
                    <SkinPicker
                        characterId={Number(objectOf(accounts[skinPlayer]?.character)?.charid ?? 0)}
                        skinId={Number(objectOf(accounts[skinPlayer]?.character)?.skin ?? accounts[skinPlayer]?.avatarId ?? 0)}
                        onClose={() => setSkinPlayer(null)}
                        onSelect={(characterId, skinId) => {
                            let next = replacePath(value, ["accounts", skinPlayer, "character", "charid"], characterId);
                            next = replacePath(next, ["accounts", skinPlayer, "character", "skin"], skinId);
                            next = replacePath(next, ["accounts", skinPlayer, "avatarId"], skinId);
                            onChange(next);
                            setSkinPlayer(null);
                        }}
                    />
                ) : null}
            </div>
        );
    }
    const wrapperPath = objectOf(value.result) ? ["result"] : [];
    const wrapper = objectOf(value.result) ?? value;
    const name = shortName(String(wrapper.name ?? ""));
    const data = objectOf(wrapper.data) ?? {};
    const dataPath = [...wrapperPath, "data"];
    const fields = EVENT_FIELDS[name];
    const huleFields = name === "RecordHule" ? HULE_FIELDS : XUEZHAN_HULE_FIELDS;
    return (
        <div className={styles.specialEditor}>
            <label className={styles.formField}>
                <span>{t("game_record.editor.event_type")}</span>
                <select value={String(wrapper.name ?? "")} onChange={(event) => {
                    const eventName = shortName(event.target.value);
                    let next = replacePath(value, [...wrapperPath, "name"], event.target.value);
                    next = replacePath(next, dataPath, eventName === "RecordHule" ? DEFAULT_HULE_DATA : {});
                    onChange(withEventDefaults(next));
                }}>
                    {Object.keys(EVENT_FIELDS).map((eventName) => <option key={eventName} value={`.lq.${eventName}`}>{t(`game_record.action.${eventName}`, {defaultValue: eventName})}</option>)}
                </select>
            </label>
            {fields ? <div className={styles.formGrid}>{fields.map((spec) => <FieldInput key={spec.key} spec={spec} value={data[spec.key]} onChange={(next) => onChange(replacePath(value, [...dataPath, spec.key], next))}/>)}</div> : null}
            {name.includes("Hule") ? arrayOf(data.hules).map((rawHule, index) => {
                const hule = objectOf(rawHule);
                return hule ? (
                    <fieldset key={index} className={`${styles.editorGroup} ${styles.winnerEditor}`}>
                        <legend>{t("game_record.editor.winner", {index: index + 1})}</legend>
                        <div className={styles.formGrid}>{huleFields.map((spec) => <FieldInput key={spec.key} spec={spec} value={hule[spec.key]} onChange={(next) => onChange(replacePath(value, [...dataPath, "hules", index, spec.key], next))}/>)}</div>
                        {name === "RecordHule" ? <button type="button" onClick={() => onChange(replacePath(value, [...dataPath, "hules"], arrayOf(data.hules).filter((_, huleIndex) => huleIndex !== index)))}>{t("game_record.editor.remove_winner")}</button> : null}
                    </fieldset>
                ) : null;
            }) : null}
            {name === "RecordHule" ? <button className={styles.addWinnerButton} type="button" onClick={() => onChange(replacePath(value, [...dataPath, "hules"], [...arrayOf(data.hules), {...DEFAULT_HULE}]))}>{t("game_record.editor.add_winner")}</button> : null}
        </div>
    );
}

function JsonEditor({value, kind, onSave, onCancel}: {value: ObjectValue; kind: "head" | "event"; onSave: (value: ObjectValue) => void; onCancel: () => void}) {
    const {t} = useTranslation();
    const [draft, setDraft] = React.useState(() => JSON.stringify(kind === "event" ? withEventDefaults(value) : value, null, 2));
    const parsed = React.useMemo(() => {
        try { return objectOf(JSON.parse(draft)); } catch { return null; }
    }, [draft]);
    const save = () => {
        try {
            const parsed = JSON.parse(draft);
            if (!objectOf(parsed)) throw new Error("not-object");
            onSave(parsed);
        } catch {
            pushToast(t("game_record.editor.invalid_json"), "error", 3600);
        }
    };
    return (
        <div className={styles.jsonEditor}>
            {parsed ? <SpecializedEditor kind={kind} value={parsed} onChange={(next) => setDraft(JSON.stringify(next, null, 2))}/> : null}
            <details className={styles.advancedEditor}>
                <summary>{t("game_record.editor.advanced_json")}</summary>
                <textarea value={draft} onChange={(event) => setDraft(event.target.value)} spellCheck={false} aria-label={t("game_record.editor.json")}/>
            </details>
            <div className={styles.editorButtons}>
                <button type="button" onClick={onCancel}>{t("game_record.editor.cancel")}</button>
                <button type="button" onClick={save}>{t("game_record.editor.save")}</button>
            </div>
        </div>
    );
}

function ActionRow({item, players, history, onInsert, onUpdate, onDelete}: {
    item: ActionItem;
    players: ReadonlyMap<number, string>;
    history: ActionItem[];
    onInsert: (item: ActionItem) => void;
    onUpdate: (item: ActionItem, value: ObjectValue) => void;
    onDelete: (item: ActionItem) => void;
}) {
    const {t} = useTranslation();
    const [open, setOpen] = React.useState(false);
    const [editing, setEditing] = React.useState(false);
    const name = shortName(item.name);
    const tiles = actionTiles(item, history);
    const changes = name === "RecordChangeTile" ? arrayOf(item.data?.changeTileInfos).map(objectOf) : [];
    const wall = name === "RecordNewRound" ? tilesIn(item.data?.paishan) : [];
    const openingHands = name === "RecordNewRound"
        ? [0, 1, 2, 3].map((seat) => tilesIn(item.data?.[`tiles${seat}`]))
        : [];
    return (
        <article className={styles.actionCard} data-kind={name}>
            <div className={styles.actionHeader}>
                <button className={styles.actionMain} type="button" onClick={() => setOpen((value) => !value)} aria-expanded={open}>
                <span className={styles.actionIndex}>#{item.index}</span>
                <span className={`ms ${styles.actionIcon}`} aria-hidden="true">{actionIcon(name)}</span>
                <span className={styles.actionText}>
                    <strong>{actionTitle(item, t)}</strong>
                    <span>{actionDescription(item, players, t)}</span>
                </span>
                {tiles.length ? (
                    <span className={styles.tiles}>
                        {tiles.map(({tile, sideways, groupStart}, index) => (
                            <span key={`${tile}-${index}`} className={`${styles.tileSlot} ${groupStart ? styles.tileGroupStart : ""}`}>
                                <Tile tile={tile} width={30} height={40} sideways={sideways} shadow={false}/>
                            </span>
                        ))}
                    </span>
                ) : null}
                {changes.length ? (
                    <span className={styles.changeTiles}>
                        {changes.map((change, seat) => change ? (
                            <span key={seat} className={styles.changePlayer}>
                                <span className={styles.changePlayerTitle}>{t("game_record.change_player", {seat, name: players.get(seat) ?? "-"})}</span>
                                <span className={styles.changeTileRow}>
                                    <span className={styles.changeTileLabel}>{t("game_record.change_out")}</span>
                                    {tilesIn(change.outTiles).map((tile, index) => <Tile key={`${tile}-${index}`} tile={tile} width={28} height={37} shadow={false}/>)}
                                </span>
                                <span className={styles.changeTileRow}>
                                    <span className={styles.changeTileLabel}>{t("game_record.change_in")}</span>
                                    {tilesIn(change.inTiles).map((tile, index) => <Tile key={`${tile}-${index}`} tile={tile} width={28} height={37} shadow={false}/>)}
                                </span>
                            </span>
                        ) : null)}
                    </span>
                ) : null}
                <span className={`ms ${styles.chevron}`} data-open={open} aria-hidden="true">chevron_right</span>
                </button>
                <div className={styles.actionTools}>
                    <button type="button" onClick={() => onInsert(item)} aria-label={t("game_record.editor.insert_event")} title={t("game_record.editor.insert_event")}><span className="ms" aria-hidden="true">add</span></button>
                    <button type="button" onClick={() => setEditing(true)} aria-label={t("game_record.editor.edit_event")} title={t("game_record.editor.edit_event")}><span className="ms" aria-hidden="true">edit</span></button>
                    <button type="button" onClick={() => onDelete(item)} aria-label={t("game_record.editor.delete_event")} title={t("game_record.editor.delete_event")}><span className="ms" aria-hidden="true">delete</span></button>
                </div>
            </div>
            {openingHands.some((hand) => hand.length) ? (
                <div className={styles.openingHands}>
                    {openingHands.map((hand, seat) => hand.length ? (
                        <div key={seat} className={styles.openingHand}>
                            <span>{t("game_record.change_player", {seat, name: players.get(seat) ?? "-"})}</span>
                            <div>{hand.map((tile, index) => <Tile key={`${tile}-${index}`} tile={tile} width={28} height={37} shadow={false}/>)}</div>
                        </div>
                    ) : null)}
                </div>
            ) : null}
            {wall.length ? (
                <div className={styles.wall}>
                    <div className={styles.wallTiles}>
                        {wall.map((tile, index) => <Tile key={`${tile}-${index}`} tile={tile} width={28} height={37} shadow={false}/>)}
                    </div>
                </div>
            ) : null}
            {open ? (
                <div className={`selectable ${styles.actionJson}`}>
                    <JsonNode label="action" value={item.raw} root/>
                </div>
            ) : null}
            {editing ? (
                <EditorDialog title={t("game_record.editor.event_title", {index: item.index})} onClose={() => setEditing(false)}>
                    <JsonEditor kind="event" value={item.raw} onCancel={() => setEditing(false)} onSave={(value) => {
                        onUpdate(item, value);
                        setEditing(false);
                    }}/>
                </EditorDialog>
            ) : null}
        </article>
    );
}

export default function GameRecordPage({record, onChange}: {
    record: ObjectValue | null;
    onChange: (record: Record<string, backendIpc.JsonValue> | null) => void;
}) {
    const {t, i18n} = useTranslation();
    const fileInputRef = React.useRef<HTMLInputElement | null>(null);
    const [onlineUuid, setOnlineUuid] = React.useState("");
    const [onlineLoading, setOnlineLoading] = React.useState(false);
    const [overrideLoading, setOverrideLoading] = React.useState(false);
    const [overrideArmed, setOverrideArmed] = React.useState(false);
    const [editingHead, setEditingHead] = React.useState(false);
    React.useEffect(() => backendIpc.subscribeBackendEvent("game_record_override_status", setOverrideArmed), []);
    const actions = React.useMemo(() => recordActions(record), [record]);
    const groups = React.useMemo(() => groupActions(actions, t), [actions, t]);
    const [selectedKey, setSelectedKey] = React.useState("");
    const [showProtocol, setShowProtocol] = React.useState(false);
    React.useEffect(() => setSelectedKey((current) => groups.some((group) => group.key === current) ? current : groups[0]?.key ?? ""), [groups]);
    const selected = groups.find((group) => group.key === selectedKey) ?? groups[0];
    const head = objectOf(record?.head);
    const recordUuid = String(head?.uuid ?? "").trim();
    const accounts = React.useMemo(() => arrayOf(head?.accounts)
        .map(objectOf).filter((value): value is ObjectValue => Boolean(value))
        .sort((left, right) => Number(left.seat) - Number(right.seat)), [head]);
    const players = React.useMemo(() => new Map(accounts
        .map((account) => [Number(account.seat), String(account.nickname ?? "-")] as const)), [accounts]);
    const detail = objectOf(objectOf(record?.data)?.data);
    const visibleActions = selected?.actions.filter((action) => showProtocol || Boolean(action.name)) ?? [];

    const desensitizeRecord = () => {
        if (!record) return;
        const next = structuredClone(record) as Record<string, backendIpc.JsonValue>;
        const nextHead = objectOf(next.head);
        if (!nextHead) return;
        const language = (i18n.resolvedLanguage || i18n.language).startsWith("ja") ? "jp" : "chs";
        nextHead.uuid = "";
        nextHead.startTime = 0;
        nextHead.endTime = 0;
        for (const value of arrayOf(nextHead.accounts)) {
            const account = objectOf(value);
            if (!account) continue;
            account.accountId = 0;
            const skinId = Number(objectOf(account.character)?.skin ?? account.avatarId ?? 0);
            const skin = SKIN_CATALOG.find((item) => item.id === skinId);
            account.nickname = skin ? skin.name[language] || skin.name.chs : String(skinId);
        }
        onChange(next);
        pushToast(t("game_record.desensitized"), "success", 2200);
    };

    const changeHead = (value: ObjectValue) => {
        if (!record) return;
        const next = structuredClone(record) as Record<string, backendIpc.JsonValue>;
        next.head = value as backendIpc.JsonValue;
        onChange(next);
        setEditingHead(false);
    };

    const changeEvent = (item: ActionItem, value: ObjectValue | null) => {
        if (!record) return;
        const next = structuredClone(record) as Record<string, backendIpc.JsonValue>;
        const nextDetail = objectOf(objectOf(next.data)?.data);
        if (!nextDetail) return;
        const events = arrayOf(nextDetail[item.source]);
        if (value) events[item.index] = value;
        else events.splice(item.index, 1);
        nextDetail[item.source] = events;
        onChange(next);
    };

    const addEvent = (after?: ActionItem) => {
        if (!record || !detail) return;
        const next = structuredClone(record) as Record<string, backendIpc.JsonValue>;
        const nextDetail = objectOf(objectOf(next.data)?.data);
        if (!nextDetail) return;
        const source = after?.source ?? (arrayOf(detail.actions).length || !arrayOf(detail.records).length ? "actions" : "records");
        const events = arrayOf(nextDetail[source]);
        const result = {name: ".lq.RecordDealTile", data: {}};
        const insertAt = (after?.index ?? selected?.actions[selected.actions.length - 1]?.index ?? events.length - 1) + 1;
        events.splice(insertAt, 0, source === "actions" ? {result} : result);
        nextDetail[source] = events;
        onChange(next);
    };

    const overrideRecord = async () => {
        if (!record) return;
        setOverrideLoading(true);
        try {
            const result = await backendIpc.overrideGameRecord(record as backendIpc.JsonValue);
            if (result.ok) setOverrideArmed(true);
            pushToast(t(result.ok ? "game_record.override_ready" : "game_record.override_failed", {reason: result.reason ?? result.error ?? "unknown"}), result.ok ? "success" : "error", result.ok ? 2600 : 3600);
        } catch (error) {
            pushToast(t("game_record.override_failed", {reason: String(error)}), "error", 3600);
        } finally {
            setOverrideLoading(false);
        }
    };

    const exportRecord = () => {
        if (!record) return;
        const blob = new Blob([JSON.stringify(record, null, 2)], {type: "application/json"});
        const url = URL.createObjectURL(blob);
        const link = document.createElement("a");
        link.href = url;
        const fileName = String(head?.uuid ?? "").replace(/[^a-zA-Z0-9_-]/g, "_") || "game-record";
        link.download = `${fileName}.json`;
        document.body.appendChild(link);
        link.click();
        link.remove();
        window.setTimeout(() => URL.revokeObjectURL(url), 0);
    };

    const importRecord = async (event: React.ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        event.target.value = "";
        if (!file) return;
        try {
            const parsed = JSON.parse(await file.text());
            if (!objectOf(parsed)) throw new Error("record-not-object");
            onChange(parsed as Record<string, backendIpc.JsonValue>);
            pushToast(t("game_record.import_success"), "success", 2600);
        } catch {
            pushToast(t("game_record.import_failed"), "error", 3600);
        }
    };

    const importOnline = async () => {
        const uuid = onlineUuid.trim();
        if (!uuid) {
            pushToast(t("game_record.uuid_required"), "error", 2600);
            return;
        }
        setOnlineLoading(true);
        try {
            const result = await backendIpc.fetchGameRecord(uuid);
            if (result.ok) pushToast(t("game_record.online_success"), "success", 2600);
            else pushToast(t("game_record.online_failed", {reason: result.reason ?? "unknown"}), "error", 3600);
        } catch (error) {
            pushToast(t("game_record.online_failed", {reason: String(error)}), "error", 3600);
        } finally {
            setOnlineLoading(false);
        }
    };

    return (
        <div className={`${styles.wrap} ${!record ? styles.emptyLayout : ""}`}>
            <header className={styles.pageHeader}>
                <div className={styles.titleBlock}>
                    <span className={styles.titleIcon} aria-hidden="true"><span className="ms">overview</span></span>
                    <h2 className={styles.title}>{t("game_record.title")}</h2>
                </div>
            </header>
            <section className={`mj-panel ${styles.actionBar}`}>
                <div className={styles.toolbar}>
                    <button type="button" disabled={!record} onClick={() => onChange(null)}><span className="ms" aria-hidden="true">delete_sweep</span>{t("game_record.clear")}</button>
                    <button type="button" disabled={!record} onClick={desensitizeRecord}><span className="ms" aria-hidden="true">shield_lock</span>{t("game_record.desensitize")}</button>
                    <button type="button" disabled={!record} onClick={exportRecord}><span className="ms" aria-hidden="true">upload</span>{t("game_record.export")}</button>
                    <button type="button" onClick={() => fileInputRef.current?.click()}><span className="ms" aria-hidden="true">download</span>{t("game_record.import")}</button>
                    <button className={overrideArmed ? styles.overrideActive : ""} type="button" aria-pressed={overrideArmed} disabled={!record || overrideLoading} onClick={overrideRecord}><span className="ms" aria-hidden="true">published_with_changes</span>{t(overrideLoading ? "game_record.overriding" : "game_record.override")}</button>
                    <input ref={fileInputRef} className={styles.fileInput} type="file" accept=".json,application/json" onChange={importRecord}/>
                </div>
                <span className={styles.toolbarDivider} aria-hidden="true"/>
                <span className={styles.onlineImport}>
                    <span className={`ms ${styles.inputIcon}`} aria-hidden="true">link</span>
                    <input value={onlineUuid} onChange={(event) => setOnlineUuid(event.target.value)} placeholder={t("game_record.uuid_placeholder")} aria-label={t("game_record.uuid_placeholder")}/>
                    <button className={styles.onlineImportButton} type="button" disabled={onlineLoading} onClick={importOnline} aria-label={t(onlineLoading ? "game_record.importing_online" : "game_record.online_import_action")} title={t(onlineLoading ? "game_record.importing_online" : "game_record.online_import_action")}><span className="ms" aria-hidden="true">cloud_download</span></button>
                </span>
            </section>
            {!record ? <div className={`mj-panel ${styles.emptyState}`}><span className="ms" aria-hidden="true">history_edu</span><strong>{t("game_record.empty")}</strong></div> : null}
            {record ? (
                <>
                    <section className={`mj-panel ${styles.summary}`}>
                        <div className={styles.summaryIdentity}>
                            <span className="ms" aria-hidden="true">description</span>
                            <div><span>{t("game_record.uuid")}</span><strong title={recordUuid || t("game_record.none")}>{recordUuid || t("game_record.none")}</strong></div>
                        </div>
                        <div className={styles.summaryStats}>
                            <div><span>{t("game_record.players")}</span><strong>{players.size}</strong></div>
                            <div><span>{t("game_record.rounds")}</span><strong>{groups.filter((group) => group.key !== "setup").length}</strong></div>
                            <div><span>{t("game_record.actions")}</span><strong>{actions.length}</strong></div>
                            <div><span>{t("game_record.records")}</span><strong>{actions.filter((action) => action.name).length}</strong></div>
                            <div><span>{t("game_record.version")}</span><strong>{String(detail?.version ?? "-")}</strong></div>
                        </div>
                    </section>
                    {editingHead ? (
                        <EditorDialog title={t("game_record.editor.head")} onClose={() => setEditingHead(false)}>
                            <JsonEditor kind="head" value={head ?? {}} onCancel={() => setEditingHead(false)} onSave={changeHead}/>
                        </EditorDialog>
                    ) : null}
                    <section className={`mj-panel ${styles.accountSection}`}>
                        <div className={styles.editorHeading}>
                            <div className={styles.sectionTitle}><span className="ms" aria-hidden="true">group</span><h3>{t("game_record.account_info")}</h3></div>
                            <button type="button" onClick={() => setEditingHead(true)}><span className="ms" aria-hidden="true">edit</span>{t("game_record.editor.edit_head")}</button>
                        </div>
                        <div className={styles.accounts}>
                            {accounts.map((account) => {
                                const character = objectOf(account.character);
                                return (
                                    <article key={String(account.accountId ?? account.seat)} className={styles.accountCard}>
                                        <header><span className={styles.seatBadge}>{String(account.seat ?? "-")}</span><strong>{String(account.nickname ?? "-")}</strong><span>{t("game_record.seat_number", {seat: account.seat ?? "-"})}</span></header>
                                        <dl>
                                            <div><dt>{t("game_record.character_id")}</dt><dd>{String(character?.charid ?? "-")}</dd></div>
                                            <div><dt>{t("game_record.skin_id")}</dt><dd>{String(character?.skin ?? "-")}</dd></div>
                                            <div><dt>{t("game_record.upgraded")}</dt><dd>{character?.isUpgraded == null ? "-" : t(character.isUpgraded === true ? "game_record.yes" : "game_record.no")}</dd></div>
                                            <div><dt>{t("game_record.character_level")}</dt><dd>{String(character?.level ?? "-")}</dd></div>
                                        </dl>
                                    </article>
                                );
                            })}
                        </div>
                    </section>
                    <section className={`mj-panel ${styles.timelinePanel}`}>
                    <nav className={styles.roundTabs} aria-label={t("game_record.rounds")}>
                        {groups.map((group) => (
                            <button key={group.key} type="button" className={group.key === selected?.key ? styles.active : ""} onClick={() => setSelectedKey(group.key)}>
                                <span>{group.label}</span><small>{group.actions.length}</small>
                            </button>
                        ))}
                    </nav>
                    <div className={styles.timelineHeading}>
                        <div className={styles.sectionTitle}><span className="ms" aria-hidden="true">timeline</span><strong>{selected?.label}</strong></div>
                        <div className={styles.timelineTools}>
                            <button type="button" onClick={() => addEvent()}><span className="ms" aria-hidden="true">add</span>{t("game_record.editor.add_event")}</button>
                            <label><input type="checkbox" checked={showProtocol} onChange={(event) => setShowProtocol(event.target.checked)}/>{t("game_record.show_protocol")}</label>
                        </div>
                    </div>
                    <section className={styles.timeline}>
                        {visibleActions.map((item) => <ActionRow key={`${item.source}-${item.index}`} item={item} players={players} history={selected?.actions ?? []} onInsert={addEvent} onUpdate={changeEvent} onDelete={(value) => changeEvent(value, null)}/>)}
                    </section>
                    </section>
                </>
            ) : null}
        </div>
    );
}
