import "../styles/theme.css";
import React, {useEffect, useRef, useState} from "react";
import {ws} from "../lib/ws";
import styles from "./SettingsPage.module.css";
import {pushToast} from "../lib/toast";
import LanguageSwitcher from "../components/LanguageSwitcher";
import {t} from "i18next";

type Tables = Record<string, Record<string, any>>;

function deepEqual(a: any, b: any) {
    try {
        return JSON.stringify(a) === JSON.stringify(b);
    } catch {
        return false;
    }
}

export default function SettingsPage() {
    const [serverTables, setServerTables] = useState<Tables | null>(null);
    const [draft, setDraft] = useState<Tables>({});
    const [dirty, setDirty] = useState(false);
    const [hasServerUpdateWhileDirty, setHasServerUpdateWhileDirty] = useState(false);
    const awaitingSyncRef = useRef(false);

    useEffect(() => {
        ws.connect();
        const off = ws.on((pkt: any) => {
            if (pkt.type === "update_config") {
                const incoming: Tables = pkt.data || {};
                setServerTables(incoming);
                if (awaitingSyncRef.current && deepEqual(incoming, draft)) {
                    awaitingSyncRef.current = false;
                    setDirty(false);
                    setHasServerUpdateWhileDirty(false);
                    pushToast(t("settings.toast_save_done"), "success", 1800);
                } else if (!dirty) {
                    setDraft(incoming);
                } else {
                    setHasServerUpdateWhileDirty(true);
                }
            } else if (pkt.type === "open_result") {
                if (pkt.data?.ok) pushToast(t("settings.toast_open_config_ok"), "info", 1500);
                else pushToast(t("settings.toast_open_config_fail", {error: pkt.data?.error || ""}), "error");
            }
        });
        const timeout = setTimeout(() => {
            ws.send({type: "request_update", data: {}} as any);
        }, 200);
        return () => {
            off();
            clearTimeout(timeout);
        };
    }, []);

    const onChange = (tname: string, key: string, val: any) => {
        setDirty(true);
        setDraft(prev => ({...prev, [tname]: {...(prev[tname] ?? {}), [key]: val}}));
    };

    const SAVE_COOLDOWN_MS = 800;
    const lastSaveRef = useRef(0);
    const [saving, setSaving] = useState(false);
    const save = () => {
        const now = Date.now();
        if (now - lastSaveRef.current < SAVE_COOLDOWN_MS || saving) return;
        lastSaveRef.current = now;
        setSaving(true);
        awaitingSyncRef.current = true;
        ws.send({type: "edit_config", data: draft} as any);
        pushToast(t("settings.toast_save_submitted"), "info", 1200);
        setTimeout(() => setSaving(false), 600);
    };

    const trKey = (table: string, key: string) => {
        const nameKey = `settings.config.${table}.${key}`;
        const descKey = `settings.config.${table}.${key}_desc`;
        return { nameKey, descKey };
    };

    const discard = () => {
        if (serverTables) setDraft(serverTables);
        setDirty(false);
        setHasServerUpdateWhileDirty(false);
        awaitingSyncRef.current = false;
        pushToast(t("settings.toast_discard"), "info", 1200);
    };
    const manualSync = () => ws.send({type: "request_update", data: {}} as any);
    const openConfigDir = () => ws.send({type: "open_config_dir", data: {}} as any);

    if (!serverTables && !Object.keys(draft).length) {
        return <div className="settings-wrap">{t("settings.loading")}</div>;
    }

    const tablesToRender = Object.keys(draft).length ? draft : (serverTables ?? {});
    const sections = Object.entries(tablesToRender);

    return (
        <div className="settings-wrap">
            <div className="settings-header">
                <h2 className="title">{t("settings.title")}</h2>
                <div className="toolbar">
                    <div className={styles.langWrap}>
                        <span>{t("settings.language_label")}</span>
                        <LanguageSwitcher />
                    </div>
                    <button className="btn" onClick={manualSync}>{t("settings.btn_manual_sync")}</button>
                    <button className="btn" onClick={openConfigDir}>{t("settings.btn_open_config_dir")}</button>
                </div>
            </div>

            {dirty && hasServerUpdateWhileDirty && (
                <div className="notice">
                    {t("settings.notice_server_update_dirty")}
                </div>
            )}

            <div className={`mj-panel card ${styles.bigCard}`}>
                {sections.map(([tname, kv], idx) => (
                    <div key={tname} className={styles.section}>
                        <h3
                            className={styles.sectionTitle}
                        >
                            {t(`settings.table.${tname}`, { defaultValue: `settings.table.${tname}` })}
                        </h3>
                        <div className="rows">
                            {Object.entries(kv).map(([key, val]) => {
                                const id = `${tname}.${key}`;
                                const { nameKey, descKey } = trKey(tname, key);

                                const input =
                                    typeof val === "boolean" ? (
                                        <input
                                            id={id}
                                            type="checkbox"
                                            className="form-checkbox"
                                            checked={!!val}
                                            onChange={(e) => onChange(tname, key, e.target.checked)}
                                            title={descKey}
                                        />
                                    ) : typeof val === "number" ? (
                                        <input
                                            id={id}
                                            type="number"
                                            className="form-input"
                                            value={val}
                                            onChange={(e) => onChange(tname, key, Number(e.target.value))}
                                            title={descKey}
                                        />
                                    ) : (
                                        <input
                                            id={id}
                                            className="form-input"
                                            value={val ?? ""}
                                            onChange={(e) => onChange(tname, key, e.target.value)}
                                            title={descKey}
                                        />
                                    );

                                return (
                                    <div className="row" key={key}>
                                        <label htmlFor={id} title={t(descKey)}>{t(nameKey)}</label>
                                        {input}
                                    </div>
                                );
                            })}
                        </div>
                        {idx < sections.length - 1 && <hr className={styles.divider}/>}
                    </div>
                ))}
            </div>

            <div className="actions">
                <button className={`btn ${saving ? "loading" : ""}`} onClick={save} disabled={!ws.connected || saving}>
                    {saving ? t("settings.btn_saving") : t("settings.btn_save")}
                </button>
                <button className="btn ghost" onClick={discard} disabled={!dirty}>{t("settings.btn_discard")}</button>
            </div>
        </div>
    );
}