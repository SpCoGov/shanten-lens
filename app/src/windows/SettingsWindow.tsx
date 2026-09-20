import "../styles/theme.css";
import {useEffect, useMemo, useRef, useState} from "react";
import {getCurrentWindow} from "@tauri-apps/api/window";
import * as backendIpc from "../lib/ipc";
import styles from "./SettingsWindow.module.css";
import LanguageSwitcher from "../components/LanguageSwitcher";
import {useTranslation} from "react-i18next";
import {pushToast} from "../lib/toast";
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

function mergeTables(base: Tables, patch: Tables): Tables {
    const merged = {...base};
    for (const [table, fields] of Object.entries(patch)) merged[table] = {...merged[table], ...fields};
    return merged;
}

export default function SettingsWindow() {
    const {t, i18n} = useTranslation();
    const appWindow = useMemo(() => getCurrentWindow(), []);

    const [serverTables, setServerTables] = useState<Tables | null>(null);
    const [draft, setDraft] = useState<Tables>({});
    const [active, setActive] = useState<string | null>(null);
    const [tileSkinState, setTileSkinState] = useState<TileSkin>(readTileSkin());
    const [useSystemProxy, setUseSystemProxy] = useState(() => readUpdatePrefs().useSystemProxy);

    const pending = useRef<Tables>({});
    const server = useRef<Tables>({});
    const inFlight = useRef<Promise<void> | null>(null);
    const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const closing = useRef(false);

    const accept = (incoming: Tables) => {
        server.current = incoming;
        setServerTables(incoming);
        setDraft(mergeTables(incoming, pending.current));
        setActive((current) => current && current in incoming
            ? current : (SETTINGS_TABLES.find((name) => name in incoming) ?? null));
    };
    const flush = async () => {
        if (saveTimer.current) clearTimeout(saveTimer.current);
        while (inFlight.current) await inFlight.current;
        if (!Object.keys(pending.current).length) return;
        const patch = mergeTables({}, pending.current);
        const save = backendIpc.updateConfig(patch).then(() => {
            for (const [table, fields] of Object.entries(patch)) {
                for (const [key, value] of Object.entries(fields)) {
                    if (deepEqual(pending.current[table]?.[key], value)) delete pending.current[table][key];
                }
                if (!Object.keys(pending.current[table] ?? {}).length) delete pending.current[table];
            }
            accept(mergeTables(server.current, patch));
        });
        inFlight.current = save;
        try { await save; } finally { inFlight.current = null; }
        if (Object.keys(pending.current).length) await flush();
    };
    const saveError = (error: unknown) => pushToast(t("settings.save_failed", {reason: String(error)}), "error", 5000);
    const close = async () => {
        try { await flush(); closing.current = true; await appWindow.close(); }
        catch (error) { closing.current = false; saveError(error); }
    };

    useEffect(() => {
        let active = true;
        const unsubscribe = backendIpc.subscribeBackendEvent("update_config", (config) => accept(config as Tables));
        void backendIpc.initializeBackend().then((snapshot) => { if (active) accept(snapshot.config as Tables); }).catch(saveError);
        const closeListener = appWindow.onCloseRequested((event) => {
            if (closing.current) return;
            event.preventDefault();
            void close();
        });
        return () => { active = false; unsubscribe(); void closeListener.then((unlisten) => unlisten()); };
    }, []);

    useEffect(() => {
        if (!Object.keys(pending.current).length) return;
        saveTimer.current = setTimeout(() => { void flush().catch(saveError); }, 600);
        return () => { if (saveTimer.current) clearTimeout(saveTimer.current); };
    }, [draft]);

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
        pending.current = mergeTables(pending.current, {[tname]: {[key]: val}});
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

    const onSync = () => void backendIpc.initializeBackend().then((snapshot) => {
        accept(snapshot.config as Tables);
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
                    <button className={styles.iconBtn} onClick={() => void close()} title={t("window.close") as string}>
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
