import React from "react";
import {useTranslation} from "react-i18next";
import Tile from "../components/Tile";
import {
    YAKU_LABELS,
    buildFeedback,
    generateTodayPuzzle,
    handCalcKey,
    hasTileOverflow,
    validateGuess,
    type FeedbackColor,
    type HandAnalysis,
    type TodayTile,
} from "../lib/todayWin";
import styles from "./TodayWinPage.module.css";

type GuessRecord = {
    tiles: TodayTile[];
    analysis: HandAnalysis;
    colors: FeedbackColor[];
    waitMatches: boolean;
    winTileMatches: boolean;
};

type DateHistory = Record<string, "won" | "failed" | "played">;

const SUIT_GROUPS: Array<{ label: string; tiles: TodayTile[] }> = [
    {label: "m", tiles: ["1m", "2m", "3m", "4m", "5m", "0m", "6m", "7m", "8m", "9m"]},
    {label: "p", tiles: ["1p", "2p", "3p", "4p", "5p", "0p", "6p", "7p", "8p", "9p"]},
    {label: "s", tiles: ["1s", "2s", "3s", "4s", "5s", "0s", "6s", "7s", "8s", "9s"]},
    {label: "z", tiles: ["1z", "2z", "3z", "4z", "5z", "6z", "7z"]},
];

const HISTORY_KEY = "today-win:history:v1";

function readHistory(): DateHistory {
    try {
        const raw = localStorage.getItem(HISTORY_KEY);
        return raw ? JSON.parse(raw) as DateHistory : {};
    } catch {
        return {};
    }
}

function writeHistory(history: DateHistory) {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
}

function mergeHistoryStatus(current: DateHistory[string] | undefined, next: DateHistory[string]) {
    const priority: Record<DateHistory[string], number> = {played: 1, failed: 2, won: 3};
    return current && priority[current] > priority[next] ? current : next;
}

function monthKey(dateKey: string) {
    return dateKey.slice(0, 7);
}

function addMonths(yyyyMm: string, delta: number) {
    const [year, month] = yyyyMm.split("-").map(Number);
    const date = new Date(year, month - 1 + delta, 1);
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`;
}

function addYears(yyyyMm: string, delta: number) {
    const year = Number(yyyyMm.slice(0, 4)) + delta;
    return `${year}-${yyyyMm.slice(5, 7)}`;
}

function buildMonthDays(yyyyMm: string) {
    const [year, month] = yyyyMm.split("-").map(Number);
    const first = new Date(year, month - 1, 1);
    const count = new Date(year, month, 0).getDate();
    return [
        ...Array.from({length: first.getDay()}, () => ""),
        ...Array.from({length: count}, (_, index) => `${yyyyMm}-${String(index + 1).padStart(2, "0")}`),
    ];
}

function tilesSameSet(a: TodayTile[], b: TodayTile[]) {
    return handCalcKey(a) === handCalcKey(b);
}

function countTiles(tiles: TodayTile[]) {
    const counts = new Map<TodayTile, number>();
    tiles.forEach((tile) => counts.set(tile, (counts.get(tile) ?? 0) + 1));
    return counts;
}

function validateHardMode(previous: GuessRecord | undefined, nextTiles: TodayTile[]) {
    if (!previous) return "";
    const nextCounts = countTiles(nextTiles);
    const requiredCounts = new Map<TodayTile, number>();
    const grayCounts = new Map<TodayTile, number>();
    for (let index = 0; index < previous.tiles.length; index++) {
        const tile = previous.tiles[index];
        const color = previous.colors[index];
        if (color === "green" && index === 13 && nextTiles[index] !== tile) {
            return "today_win.errors.hard_green";
        }
        if (color === "green" || color === "yellow") {
            requiredCounts.set(tile, (requiredCounts.get(tile) ?? 0) + 1);
        }
        if (color === "gray") {
            grayCounts.set(tile, (grayCounts.get(tile) ?? 0) + 1);
        }
    }
    for (const [tile, required] of requiredCounts) {
        if ((nextCounts.get(tile) ?? 0) < required) return "today_win.errors.hard_marked";
    }
    for (const [tile] of grayCounts) {
        const confirmed = requiredCounts.get(tile) ?? 0;
        if ((nextCounts.get(tile) ?? 0) > confirmed) return "today_win.errors.hard_gray";
    }
    return "";
}

function TileRun({
                     tiles,
                     colors,
                     hidden = false,
                 }: {
    tiles: TodayTile[];
    colors?: FeedbackColor[];
    hidden?: boolean;
}) {
    const shown = hidden ? Array(14).fill(null) : tiles;
    return (
        <div className={styles.handRow}>
            {shown.slice(0, 13).map((tile, index) => (
                <div key={`hand-${index}`} className={colors ? `${styles.feedback} ${styles[colors[index]]}` : undefined}>
                    {tile ? <Tile tile={tile} width={30} height={40}/> : <div className={styles.tileSlot}/>}
                </div>
            ))}
            <span className={styles.winGap} aria-hidden="true"/>
            <div className={colors ? `${styles.feedback} ${styles[colors[13]]}` : undefined}>
                {shown[13] ? <Tile tile={shown[13]} width={30} height={40}/> : <div className={styles.tileSlot}/>}
            </div>
        </div>
    );
}

export default function TodayWinPage() {
    const {t} = useTranslation();
    const todayKey = React.useMemo(() => generateTodayPuzzle().dateKey, []);
    const [dateKey, setDateKey] = React.useState(todayKey);
    const puzzle = React.useMemo(() => generateTodayPuzzle(dateKey), [dateKey]);
    const [concealed, setConcealed] = React.useState<TodayTile[]>([]);
    const [winTile, setWinTile] = React.useState<TodayTile | null>(null);
    const [guesses, setGuesses] = React.useState<GuessRecord[]>([]);
    const [messageKey, setMessageKey] = React.useState("");
    const [won, setWon] = React.useState(false);
    const [hardMode, setHardMode] = React.useState(false);
    const [resultOpen, setResultOpen] = React.useState(false);
    const [calendarOpen, setCalendarOpen] = React.useState(false);
    const [visibleMonth, setVisibleMonth] = React.useState(monthKey(todayKey));
    const [history, setHistory] = React.useState<DateHistory>(() => readHistory());
    const calendarRef = React.useRef<HTMLDivElement | null>(null);

    const gameOver = won || guesses.length >= 6;
    const answerShown = gameOver;
    const answerDisplayTiles = React.useMemo(() => [...puzzle.concealed, puzzle.winTile], [puzzle.concealed, puzzle.winTile]);
    const inputTiles = React.useMemo(() => [...concealed, ...(winTile ? [winTile] : [])], [concealed, winTile]);

    React.useEffect(() => {
        setConcealed([]);
        setWinTile(null);
        setGuesses([]);
        setMessageKey("");
        setWon(false);
        setResultOpen(false);
        setCalendarOpen(false);
        setVisibleMonth(monthKey(dateKey));
    }, [dateKey]);

    React.useEffect(() => {
        if (!calendarOpen) return;
        const onPointerDown = (event: PointerEvent) => {
            if (!calendarRef.current?.contains(event.target as Node)) setCalendarOpen(false);
        };
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key === "Escape") setCalendarOpen(false);
        };
        document.addEventListener("pointerdown", onPointerDown);
        document.addEventListener("keydown", onKeyDown);
        return () => {
            document.removeEventListener("pointerdown", onPointerDown);
            document.removeEventListener("keydown", onKeyDown);
        };
    }, [calendarOpen]);

    const setError = React.useCallback((key: string) => setMessageKey(key), []);

    const addTile = React.useCallback((tile: TodayTile) => {
        if (gameOver) return;
        if (hasTileOverflow([...inputTiles, tile])) {
            setError("today_win.errors.tile_overflow");
            return;
        }
        if (concealed.length < 13) {
            setConcealed((current) => [...current, tile]);
            setMessageKey("");
            return;
        }
        if (!winTile) {
            setWinTile(tile);
            setMessageKey("");
            return;
        }
        setError("today_win.errors.too_many_hand");
    }, [concealed.length, gameOver, inputTiles, setError, winTile]);

    const deleteTile = React.useCallback(() => {
        if (gameOver) return;
        setMessageKey("");
        if (winTile) {
            setWinTile(null);
            return;
        }
        setConcealed((current) => current.slice(0, -1));
    }, [gameOver, winTile]);

    const clearInput = React.useCallback(() => {
        if (gameOver) return;
        setConcealed([]);
        setWinTile(null);
        setMessageKey("");
    }, [gameOver]);

    const submit = React.useCallback(() => {
        if (gameOver) return;
        const orderedTiles = [...concealed, ...(winTile ? [winTile] : [])];
        const validation = validateGuess(concealed, winTile, puzzle.waits);
        if (!validation.ok) {
            setError(validation.errorKey);
            return;
        }
        if (hardMode) {
            const hardModeError = validateHardMode(guesses[guesses.length - 1], orderedTiles);
            if (hardModeError) {
                setError(hardModeError);
                return;
            }
        }
        const colors = buildFeedback(orderedTiles, answerDisplayTiles, validation.analysis, puzzle.analysis, winTile as TodayTile, puzzle.winTile);
        const winTileMatches = handCalcKey([winTile as TodayTile]) === handCalcKey([puzzle.winTile]);
        if (!validation.waitMatches && winTileMatches) {
            colors[13] = "yellow";
        }
        const exact = tilesSameSet(orderedTiles, puzzle.answer) && validation.waitMatches && winTileMatches;
        const nextGuesses = [...guesses, {tiles: orderedTiles, analysis: validation.analysis, colors, waitMatches: validation.waitMatches, winTileMatches}];
        setGuesses(nextGuesses);
        setConcealed([]);
        setWinTile(null);
        setWon(exact);
        if (exact) setResultOpen(true);
        const nextStatus: DateHistory[string] = exact ? "won" : nextGuesses.length >= 6 ? "failed" : "played";
        const nextHistory = {...history, [dateKey]: mergeHistoryStatus(history[dateKey], nextStatus)};
        setHistory(nextHistory);
        writeHistory(nextHistory);
        setMessageKey(exact ? "today_win.success" : validation.waitMatches ? "" : "today_win.errors.wait_mismatch");
    }, [answerDisplayTiles, concealed, dateKey, gameOver, guesses, hardMode, history, puzzle.analysis, puzzle.answer, puzzle.waits, puzzle.winTile, setError, winTile]);

    const answerYaku = puzzle.analysis.yaku.map((id) => YAKU_LABELS[id]).join(" / ");
    const tilePickerMarks = React.useMemo(() => {
        const marks = new Map<TodayTile, FeedbackColor>();
        const priority: Record<FeedbackColor, number> = {gray: 1, yellow: 2, green: 3};
        guesses.forEach((guess) => {
            guess.tiles.forEach((tile, index) => {
                const color = guess.colors[index];
                const current = marks.get(tile);
                if (!current || priority[color] > priority[current]) marks.set(tile, color);
            });
        });
        return marks;
    }, [guesses]);
    const monthDays = React.useMemo(() => buildMonthDays(visibleMonth), [visibleMonth]);
    const visibleYear = Number(visibleMonth.slice(0, 4));
    const currentYear = Number(todayKey.slice(0, 4));
    const visibleMonthNumber = Number(visibleMonth.slice(5, 7));
    const monthLabel = t("today_win.month_label", {month: visibleMonthNumber});

    return (
        <div className={`settings-wrap wide-page ${styles.wrap}`}>
            <div className={styles.layout}>
                <div className={styles.header}>
                    <h2 className={styles.title}>{t("today_win.title")}</h2>
                    <div className={styles.headerActions}>
                        <label className={`${styles.hardToggle} ${hardMode ? styles.isOn : ""}`}>
                            <input
                                type="checkbox"
                                checked={hardMode}
                                disabled={guesses.length > 0}
                                onChange={(event) => setHardMode(event.currentTarget.checked)}
                            />
                            <span>{t("today_win.hard_mode")}</span>
                        </label>
                        <div className={styles.datePicker} ref={calendarRef}>
                            <button
                                className={styles.dateBadge}
                                title={t("today_win.pick_date")}
                                onClick={() => setCalendarOpen((open) => !open)}
                            >
                                <span className="ms" aria-hidden="true">calendar_month</span>
                                <span>{dateKey}</span>
                            </button>
                            {calendarOpen ? (
                                <div className={styles.calendarPanel}>
                                    <div className={styles.calendarHeader}>
                                        <button className="nav-btn" onClick={() => setVisibleMonth((current) => addMonths(current, -1))} aria-label={t("today_win.prev_month")}>
                                            <span className="ms" aria-hidden="true">chevron_left</span>
                                        </button>
                                        <div className={styles.calendarTitle}>
                                            <button className={styles.titleStep} onClick={() => setVisibleMonth((current) => addYears(current, -1))} aria-label={t("today_win.prev_year")}>-</button>
                                            <strong>{visibleYear}</strong>
                                            <button className={styles.titleStep} onClick={() => setVisibleMonth((current) => addYears(current, 1))} disabled={visibleYear >= currentYear} aria-label={t("today_win.next_year")}>+</button>
                                            <span>{monthLabel}</span>
                                        </div>
                                        <button className="nav-btn" onClick={() => setVisibleMonth((current) => addMonths(current, 1))} disabled={visibleMonth >= monthKey(todayKey)} aria-label={t("today_win.next_month")}>
                                            <span className="ms" aria-hidden="true">chevron_right</span>
                                        </button>
                                    </div>
                                    <div className={styles.calendarGrid}>
                                    {Array.from({length: 7}, (_, index) => (
                                        <span key={index} className={styles.calendarWeek}>
                                            {t(`today_win.weekdays.${index}`, {defaultValue: ["日", "一", "二", "三", "四", "五", "六"][index]})}
                                        </span>
                                    ))}
                                        {monthDays.map((day, index) => {
                                            const status = day ? history[day] : undefined;
                                            return day ? (
                                                <button
                                                    key={day}
                                                    className={`${styles.calendarDay} ${day === dateKey ? styles.selectedDay : ""} ${status ? styles[`day_${status}`] : ""}`}
                                                    disabled={day > todayKey}
                                                    onClick={() => setDateKey(day)}
                                                    title={status ? t(`today_win.history_${status}`) : day}
                                                >
                                                    <span>{Number(day.slice(8))}</span>
                                                </button>
                                            ) : <span key={`blank-${index}`}/>;
                                        })}
                                    </div>
                                    <div className={styles.calendarLegend}>
                                        <span><i className={styles.legendWon}/> {t("today_win.history_won")}</span>
                                        <span><i className={styles.legendFailed}/> {t("today_win.history_failed")}</span>
                                        <span><i className={styles.legendPlayed}/> {t("today_win.history_played")}</span>
                                    </div>
                                </div>
                            ) : null}
                        </div>
                    </div>
                </div>

                <div className={styles.gameGrid}>
                    <div className={styles.boardColumn}>
                        <section className={`panel ${styles.panel}`}>
                            <div className={styles.panelTitle}>{t("today_win.answer_title")}</div>
                            <TileRun tiles={answerDisplayTiles} hidden={!answerShown}/>
                            {answerShown ? (
                                <div className={styles.answerMeta}>
                                    {t("today_win.answer_yaku", {yaku: answerYaku})}
                                </div>
                            ) : null}
                        </section>

                        <section className={`panel ${styles.panel}`}>
                            <div className={styles.panelTitle}>{t("today_win.history_title")}</div>
                            <div className={styles.guessGrid}>
                                {Array.from({length: 6}).map((_, index) => {
                                    const guess = guesses[index];
                                    const isCurrent = !guess && index === guesses.length && !gameOver;
                                    return (
                                        <div className={`${styles.guessRow} ${isCurrent ? styles.currentGuessRow : ""}`} key={index}>
                                            <div className={styles.guessIndex}>{index + 1}</div>
                                            {guess ? (
                                                <TileRun tiles={guess.tiles} colors={guess.colors}/>
                                            ) : isCurrent ? (
                                                <TileRun tiles={inputTiles}/>
                                            ) : (
                                                <TileRun tiles={[]}/>
                                            )}
                                            <div className={styles.waitHint}>
                                                {guess && !guess.waitMatches && guess.winTileMatches ? t("today_win.wait_mismatch_badge") : ""}
                                            </div>
                                        </div>
                                    );
                                })}
                            </div>
                        </section>

                    </div>
                    <aside className={styles.controlColumn}>
                        <section className={`panel ${styles.panel}`}>
                            <div className={styles.inputHeader}>
                                <div className={styles.panelTitle}>{t("today_win.picker_title")}</div>
                                <div className={styles.controls}>
                                    <button className="nav-btn" onClick={deleteTile} disabled={gameOver || inputTiles.length === 0} title={t("today_win.delete")} aria-label={t("today_win.delete")}>
                                        <span className="ms" aria-hidden="true">backspace</span>
                                    </button>
                                    <button className="nav-btn" onClick={clearInput} disabled={gameOver || inputTiles.length === 0} title={t("today_win.clear")} aria-label={t("today_win.clear")}>
                                        <span className="ms" aria-hidden="true">delete_sweep</span>
                                    </button>
                                    <button className="nav-btn" onClick={submit} disabled={gameOver} title={t("today_win.submit")} aria-label={t("today_win.submit")}>
                                        <span className="ms" aria-hidden="true">check</span>
                                    </button>
                                </div>
                            </div>
                            <div className={styles.picker}>
                                {SUIT_GROUPS.map((group) => (
                                    <div className={styles.pickerGroup} key={group.label}>
                                        {group.tiles.map((tile) => (
                                            <button
                                                key={tile}
                                                className={`${styles.tileButton} ${tilePickerMarks.get(tile) ? `${styles.pickerMarked} ${styles[tilePickerMarks.get(tile)!]}` : ""}`}
                                                onClick={() => addTile(tile)}
                                                disabled={gameOver || hasTileOverflow([...inputTiles, tile])}
                                                title={tile}
                                            >
                                                <Tile tile={tile} width={30} height={40}/>
                                            </button>
                                        ))}
                                    </div>
                                ))}
                            </div>
                            <div className={`${styles.message} ${won ? styles.success : ""}`}>
                                {messageKey ? t(messageKey) : ""}
                            </div>
                        </section>
                    </aside>
                </div>
            </div>
            {resultOpen ? (
                <div className={styles.resultOverlay} role="dialog" aria-modal="true" aria-labelledby="today-win-result-title">
                    <div className={styles.resultDialog}>
                        <div className={styles.resultHeader}>
                            <div>
                                <h2 id="today-win-result-title">{t("today_win.result_title")}</h2>
                                <p>{t("today_win.result_subtitle", {count: guesses.length})}</p>
                            </div>
                            <button className="nav-btn" onClick={() => setResultOpen(false)} aria-label={t("modal.close")}>
                                <span className="ms" aria-hidden="true">close</span>
                            </button>
                        </div>
                        <div className={styles.resultStats}>
                            <div><span>{t("today_win.result_date")}</span><strong>{dateKey}</strong></div>
                            <div><span>{t("today_win.result_count")}</span><strong>{guesses.length}/6</strong></div>
                            <div><span>{t("today_win.result_mode")}</span><strong>{hardMode ? t("today_win.hard_mode_on") : t("today_win.hard_mode_off")}</strong></div>
                            <div><span>{t("today_win.result_yaku")}</span><strong>{answerYaku}</strong></div>
                        </div>
                        <section className={styles.resultSection}>
                            <div className={styles.panelTitle}>{t("today_win.result_answer")}</div>
                            <TileRun tiles={answerDisplayTiles}/>
                        </section>
                        <section className={styles.resultSection}>
                            <div className={styles.panelTitle}>{t("today_win.result_records",)}</div>
                            <div className={styles.resultRecords}>
                                {guesses.map((guess, index) => (
                                    <div className={styles.resultRecord} key={index}>
                                        <span className={styles.guessIndex}>{index + 1}</span>
                                        <TileRun tiles={guess.tiles} colors={guess.colors}/>
                                    </div>
                                ))}
                            </div>
                        </section>
                    </div>
                </div>
            ) : null}
        </div>
    );
}
