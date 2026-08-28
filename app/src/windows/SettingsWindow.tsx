import "../styles/theme.css";
import React, {useEffect, useMemo, useRef, useState} from "react";
import {getCurrentWindow} from "@tauri-apps/api/window";
import * as backendIpc from "../lib/ipc";
import styles from "./SettingsWindow.module.css";
import LanguageSwitcher from "../components/LanguageSwitcher";
import {useTranslation} from "react-i18next";
import {readTileSkin, setTileSkin, type TileSkin} from "../lib/tileSkin";
import {readUpdatePrefs, setUpdateUseSystemProxy} from "../lib/updateCheck";

type Tables = Record<string, Record<string, any>>;
const SETTINGS_TABLES = ["game", "general", "backend"] as const;

function deepEqual(a: any, b: any) {
    try {
        return JSON.stringify(a) === JSON.stringify(b);
    } catch {
        return false;
    }
}

export default function SettingsWindow() {
    const {t, i18n} = useTranslation();
    const appWindow = getCurrentWindow();

    const [serverTables, setServerTables] = useState<Tables | null>(null);
    const [draft, setDraft] = useState<Tables>({});
    const [active, setActive] = useState<string | null>(null);
    const [tileSkinState, setTileSkinState] = useState<TileSkin>(readTileSkin());
    const [useSystemProxy, setUseSystemProxy] = useState(() => readUpdatePrefs().useSystemProxy);

    const lastInputRef = useRef(0);
    const awaitingSyncRef = useRef(false);
    const saveTimer = useRef<number | null>(null);

    const IDLE_MS = 1200;
    const SAVE_DEBOUNCE = 600;

    useEffect(() => {
        const accept = (incoming: Tables) => {
            setServerTables(incoming);

            const now = Date.now();
            const idle = now - lastInputRef.current > IDLE_MS;
            const notWaiting = !awaitingSyncRef.current;
            if (idle && notWaiting) setDraft(incoming);
            awaitingSyncRef.current = false;

            setActive((current) => current && current in incoming
                ? current
                : (SETTINGS_TABLES.find((name) => name in incoming) ?? null));
        };
        void backendIpc.initializeBackend().then((snapshot) => accept(snapshot.config as Tables));
        return backendIpc.subscribeBackendEvent("update_config", (config) => accept(config as Tables));
    }, []);

    useEffect(() => {
        if (!serverTables) return;
        if (saveTimer.current) window.clearTimeout(saveTimer.current);
        saveTimer.current = window.setTimeout(() => {
            if (deepEqual(draft, serverTables)) return;
            const savedDraft = draft;
            awaitingSyncRef.current = true;
            void backendIpc.updateConfig(savedDraft).then(() => {
                setServerTables(savedDraft);
                awaitingSyncRef.current = false;
            }, () => {
                awaitingSyncRef.current = false;
            });
        }, SAVE_DEBOUNCE) as unknown as number;
        return () => {
            if (saveTimer.current) window.clearTimeout(saveTimer.current);
        };
    }, [draft, serverTables]);

    useEffect(() => {
        const sync = () => setTileSkinState(readTileSkin());
        window.addEventListener("storage", sync);
        window.addEventListener("sl:tile-skin-change", sync as EventListener);
        return () => {
            window.removeEventListener("storage", sync);
            window.removeEventListener("sl:tile-skin-change", sync as EventListener);
        };
    }, []);

    const onChange = (tname: string, key: string, val: any) => {
        lastInputRef.current = Date.now();
        setDraft((prev) => ({...prev, [tname]: {...(prev[tname] ?? {}), [key]: val}}));
    };

    const tables = useMemo(() => {
        return Object.keys(draft).length ? draft : (serverTables ?? {});
    }, [draft, serverTables]);

    const sidebarItems = useMemo(() => {
        return SETTINGS_TABLES.filter((name) => name in tables).map((name) => ({
            name,
            changed: !!serverTables && !deepEqual(tables[name], (serverTables[name] ?? {})),
        }));
    }, [tables, serverTables]);

    const trKey = (table: string, key: string) => ({
        nameKey: `settings.config.${table}.${key}`,
        descKey: `settings.config.${table}.${key}_desc`,
    });

    const onSync = () => void backendIpc.getSnapshot().then((snapshot) => {
        setServerTables(snapshot.config as Tables);
        setDraft(snapshot.config as Tables);
    });

    const content = (() => {
        if (!active) return <div className={styles.emptyPane}>{t("settings.loading")}</div>;
        const kv = tables[active] ?? {};
        const entries = Object.entries(kv);
        if (entries.length === 0) return <div className={styles.emptyPane}>{t("settings.loading")}</div>;

        return (
            <div className={styles.sectionBody} key={active}>
                <h3 className={styles.sectionTitle}>
                    {t(`settings.table.${active}`, {defaultValue: active})}
                </h3>
                <div className={styles.kvRows}>
                    {entries.map(([key, val]) => {
                        const id = `${active}.${key}`;
                        const {nameKey, descKey} = trKey(active, key);
                        const label = t(nameKey);
                        const description = i18n.exists(descKey) ? t(descKey) : "";

                        const control = typeof val === "boolean"
                            ? (
                                <label className={styles.toggle} title={description}>
                                    <input
                                        id={id}
                                        type="checkbox"
                                        checked={!!val}
                                        onChange={(e) => onChange(active, key, e.target.checked)}
                                    />
                                    <span className={styles.toggleTrack} aria-hidden="true"><span/></span>
                                </label>
                            )
                            : typeof val === "number"
                                ? (
                                    <input
                                        id={id}
                                        type="number"
                                        className="form-input"
                                        value={val}
                                        onChange={(e) => onChange(active, key, Number(e.target.value))}
                                        title={description}
                                    />
                                )
                                : (
                                    <input
                                        id={id}
                                        className="form-input"
                                        value={val ?? ""}
                                        onChange={(e) => onChange(active, key, e.target.value)}
                                        title={description}
                                    />
                                );

                        return (
                            <div className={styles.kvRow} key={key}>
                                <label className={styles.settingCopy} htmlFor={id} title={description}>
                                    <span>{label}</span>
                                    {description ? <small>{description}</small> : null}
                                </label>
                                <div className={styles.ctrl}>{control}</div>
                            </div>
                        );
                    })}
                    {active === "general" ? (
                        <div className={styles.kvRow}>
                            <label className={styles.settingCopy} htmlFor="update.useSystemProxy">
                                <span>{t("settings.use_system_proxy")}</span>
                                <small>{t("settings.use_system_proxy_desc")}</small>
                            </label>
                            <div className={styles.ctrl}>
                                <label className={styles.toggle}>
                                    <input
                                        id="update.useSystemProxy"
                                        type="checkbox"
                                        checked={useSystemProxy}
                                        onChange={(event) => {
                                            const checked = event.currentTarget.checked;
                                            setUpdateUseSystemProxy(checked);
                                            setUseSystemProxy(checked);
                                        }}
                                    />
                                    <span className={styles.toggleTrack} aria-hidden="true"><span/></span>
                                </label>
                            </div>
                        </div>
                    ) : null}
                </div>
            </div>
        );
    })();

    return (
        <div className={styles.wrap}>
            <header className={styles.header} data-tauri-drag-region>
                <div className={styles.hleft} data-tauri-drag-region>
                    <div className={styles.title}>{t("settings.title")}</div>
                </div>
                <div className={styles.hright}>
                    <div className={styles.langWrap}>
                        <LanguageSwitcher />
                    </div>
                    <label className={styles.tileSkinWrap}>
                        <span>{t("settings.tile_skin_label")}</span>
                        <select
                            value={tileSkinState}
                            onChange={(e) => {
                                const next = e.target.value as TileSkin;
                                setTileSkin(next);
                                setTileSkinState(next);
                            }}
                        >
                            <option value="classic">{t("settings.tile_skin_classic")}</option>
                            <option value="tempai-svg">{t("settings.tile_skin_tempai_svg")}</option>
                        </select>
                    </label>
                    <button className={styles.iconBtn} onClick={onSync} title={t("settings.btn_manual_sync") as string}>
                        <span className="ms" aria-hidden="true">sync</span>
                    </button>
                    <button className={styles.iconBtn} onClick={() => appWindow.close()} title={t("window.close") as string}>
                        <span className="ms" aria-hidden="true">close</span>
                    </button>
                </div>
            </header>

            <div className={styles.main}>
                <aside className={styles.sidebar}>
                    {sidebarItems.map((it) => (
                        <button
                            key={it.name}
                            className={`${styles.sideItem} ${active === it.name ? styles.active : ""}`}
                            onClick={() => setActive(it.name)}
                            title={t(`settings.table.${it.name}`, {defaultValue: it.name})}
                        >
                            <span className={styles.sideText}>
                                {t(`settings.table.${it.name}`, {defaultValue: it.name})}
                            </span>
                            {it.changed ? <i className={styles.badgeDot} /> : null}
                        </button>
                    ))}
                </aside>

                <main className={styles.content}>
                    {content}
                </main>
            </div>
        </div>
    );
}
