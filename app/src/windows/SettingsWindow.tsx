import "../styles/theme.css";
import React, {useEffect, useRef, useState, useMemo} from "react";
import {getCurrentWindow} from "@tauri-apps/api/window";
import {ws} from "../lib/ws";
import styles from "./SettingsWindow.module.css";
import LanguageSwitcher from "../components/LanguageSwitcher";
import {useTranslation} from "react-i18next";

type Tables = Record<string, Record<string, any>>;

function deepEqual(a: any, b: any) {
    try {
        return JSON.stringify(a) === JSON.stringify(b);
    } catch {
        return false;
    }
}

export default function SettingsWindow() {
    const {t} = useTranslation();
    const appWindow = getCurrentWindow();

    const [serverTables, setServerTables] = useState<Tables | null>(null);
    const [draft, setDraft] = useState<Tables>({});
    const [active, setActive] = useState<string | null>(null); // 当前选中的表

    const lastInputRef = useRef(0);
    const awaitingSyncRef = useRef(false);
    const saveTimer = useRef<number | null>(null);

    const IDLE_MS = 1200;
    const SAVE_DEBOUNCE = 600;

    useEffect(() => {
        ws.connect();
        const off = ws.on((pkt: any) => {
            if (pkt.type === "update_config") {
                const incoming: Tables = pkt.data || {};
                setServerTables(incoming);

                const now = Date.now();
                const idle = now - lastInputRef.current > IDLE_MS;
                const notWaiting = !awaitingSyncRef.current;

                if (idle && notWaiting) setDraft(incoming);
                awaitingSyncRef.current = false;

                if (!active) {
                    const first = Object.keys(incoming)[0];
                    if (first) setActive(first);
                }
            }
        });
        const timer = window.setTimeout(() => ws.send({type: "request_update", data: {}} as any), 100);
        return () => {
            off();
            window.clearTimeout(timer);
        };
    }, [active]);

    useEffect(() => {
        if (!serverTables) return;
        if (saveTimer.current) window.clearTimeout(saveTimer.current);
        saveTimer.current = window.setTimeout(() => {
            if (!deepEqual(draft, serverTables)) {
                awaitingSyncRef.current = true;
                ws.send({type: "edit_config", data: draft} as any);
            }
        }, SAVE_DEBOUNCE) as unknown as number;
        return () => {
            if (saveTimer.current) window.clearTimeout(saveTimer.current);
        };
    }, [draft, serverTables]);

    const onChange = (tname: string, key: string, val: any) => {
        lastInputRef.current = Date.now();
        setDraft(prev => ({...prev, [tname]: {...(prev[tname] ?? {}), [key]: val}}));
    };

    const tables = useMemo(() => {
        const src: Tables = Object.keys(draft).length ? draft : (serverTables ?? {});
        return src;
    }, [draft, serverTables]);

    const sidebarItems = useMemo(() => {
        const items = Object.keys(tables);
        return items.map(name => {
            const changed =
                !!serverTables &&
                !deepEqual(tables[name], (serverTables[name] ?? {}));
            return {name, changed};
        });
    }, [tables, serverTables]);

    const trKey = (table: string, key: string) => ({
        nameKey: `settings.config.${table}.${key}`,
        descKey: `settings.config.${table}.${key}_desc`,
    });

    const onSync = () => ws.send({type: "request_update", data: {}} as any);

    const content = (() => {
        if (!active) return <div className={styles.emptyPane}>{t("settings.loading")}</div>;
        const kv = tables[active] ?? {};
        const entries = Object.entries(kv);
        if (entries.length === 0) return <div className={styles.emptyPane}>{t("settings.loading")}</div>;

        return (
            <div className={styles.sectionBody}>
                <h3 className={styles.sectionTitle}>
                    {t(`settings.table.${active}`, {defaultValue: active})}
                </h3>
                <div className={styles.kvRows}>
                    {entries.map(([key, val]) => {
                        const id = `${active}.${key}`;
                        const { nameKey, descKey } = trKey(active, key);
                        const label = t(nameKey);
                        const title = t(descKey);

                        const control = typeof val === "boolean"
                            ? (<input id={id} type="checkbox" className="form-checkbox"
                                      checked={!!val}
                                      onChange={e => onChange(active, key, e.target.checked)}
                                      title={title} />)
                            : typeof val === "number"
                                ? (<input id={id} type="number" className="form-input"
                                          value={val}
                                          onChange={e => onChange(active, key, Number(e.target.value))}
                                          title={title} />)
                                : (<input id={id} className="form-input"
                                          value={val ?? ""}
                                          onChange={e => onChange(active, key, e.target.value)}
                                          title={title} />);

                        return (
                            <div className={styles.kvRow} key={key}>
                                <label htmlFor={id} title={title}>{label}</label>
                                <div className={styles.ctrl}>{control}</div>
                            </div>
                        );
                    })}
                </div>
            </div>
        );
    })();

    return (
        <div className={styles.wrap}>
            <header className={styles.header} data-tauri-drag-region>
                <div className={styles.hleft} data-tauri-drag-region>
                    <div className={styles.title}>{t("settings.title")}</div>
                    <div className={styles.langWrap}>
                        <LanguageSwitcher/>
                    </div>
                </div>
                <div className={styles.hright}>
                    <button className={styles.iconBtn} onClick={onSync} title={t("settings.btn_manual_sync") as string}>
                        <span className="ms">sync</span>
                    </button>
                    <button className={styles.iconBtn} onClick={() => appWindow.close()} title={t("window.close") as string}>
                        <span className="ms">close</span>
                    </button>
                </div>
            </header>

            <div className={styles.main}>
                <aside className={styles.sidebar}>
                    {sidebarItems.map(it => (
                        <button
                            key={it.name}
                            className={`${styles.sideItem} ${active === it.name ? styles.active : ""}`}
                            onClick={() => setActive(it.name)}
                            title={t(`settings.table.${it.name}`, {defaultValue: it.name})}
                        >
              <span className={styles.sideText}>
                {t(`settings.table.${it.name}`, {defaultValue: it.name})}
              </span>
                            {it.changed && <i className={styles.badgeDot}/>}
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
