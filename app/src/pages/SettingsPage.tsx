import React, {useEffect, useRef, useState} from "react";
import {ws} from "../lib/ws";
import styles from "./SettingsPage.module.css";

type Tables = Record<string, Record<string, any>>;

function deepEqual(a: any, b: any) {
    try {
        return JSON.stringify(a) === JSON.stringify(b);
    } catch {
        return false;
    }
}

type ToastKind = "info" | "success" | "error";

export default function SettingsPage() {
    const [serverTables, setServerTables] = useState<Tables | null>(null);
    const [draft, setDraft] = useState<Tables>({});
    const [dirty, setDirty] = useState(false);
    const [hasServerUpdateWhileDirty, setHasServerUpdateWhileDirty] = useState(false);
    const awaitingSyncRef = useRef(false);

    // 顶部通知
    const [toast, setToast] = useState<{ msg: string; kind: ToastKind; id: number } | null>(null);
    const [toastVisible, setToastVisible] = useState(false);
    const toastTimerRef = useRef<number | null>(null);
    const pushToast = (msg: string, kind: ToastKind = "info", duration = 2200) => {
        setToast({msg, kind, id: Date.now()});
        setToastVisible(true);
        if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
        toastTimerRef.current = window.setTimeout(() => setToastVisible(false), duration);
    };
    useEffect(() => () => {
        if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    }, []);

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
                    pushToast("保存完成", "success", 1800);
                } else if (!dirty) {
                    setDraft(incoming);
                } else {
                    setHasServerUpdateWhileDirty(true);
                }
            } else if (pkt.type === "open_result") {
                if (pkt.data?.ok) pushToast("已请求打开配置目录", "info", 1500);
                else pushToast(`打开失败：${pkt.data?.error || ""}`, "error");
            }
        });
        const t = setTimeout(() => {
            ws.send({type: "request_update", data: {}} as any);
        }, 200);
        return () => {
            off();
            clearTimeout(t);
        };
    }, []);

    const onChange = (tname: string, key: string, val: any) => {
        setDirty(true);
        setDraft(prev => ({...prev, [tname]: {...(prev[tname] ?? {}), [key]: val}}));
    };

    // 保存（防抖）
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
        pushToast("已提交保存", "info", 1200);
        setTimeout(() => setSaving(false), 600);
    };

    const discard = () => {
        if (serverTables) setDraft(serverTables);
        setDirty(false);
        setHasServerUpdateWhileDirty(false);
        awaitingSyncRef.current = false;
        pushToast("已放弃未保存更改", "info", 1200);
    };
    const manualSync = () => ws.send({type: "request_update", data: {}} as any);
    const openConfigDir = () => ws.send({type: "open_config_dir", data: {}} as any);

    if (!serverTables && !Object.keys(draft).length) {
        return <div className="settings-wrap">加载配置...</div>;
    }

    const tablesToRender = Object.keys(draft).length ? draft : (serverTables ?? {});
    const sections = Object.entries(tablesToRender);

    return (
        <div className="settings-wrap">
            {/* 顶部下滑通知（全局样式） */}
            <div className={`toast ${toastVisible ? "visible" : ""} ${toast?.kind || "info"}`}>{toast?.msg}</div>

            <div className="settings-header">
                <h2 className="title">设置</h2>
                <div className="toolbar">
                    <button className="btn" onClick={manualSync}>手动同步</button>
                    <button className="btn" onClick={openConfigDir}>打开配置目录</button>
                </div>
            </div>

            {dirty && hasServerUpdateWhileDirty && (
                <div className="notice">
                    服务器有新配置推送，但你有未保存修改。你可以<span className="selectable">保存</span>或<span className="selectable">放弃更改</span>再同步。
                </div>
            )}

            <div className={`mj-panel card ${styles.bigCard}`}>
                {sections.map(([tname, kv], idx) => (
                    <div key={tname} className={styles.section}>
                        <h3 className={styles.sectionTitle}>{tname}</h3>
                        <div className="rows">
                            {Object.entries(kv).map(([key, val]) => {
                                const id = `${tname}.${key}`;
                                const input =
                                    typeof val === "boolean" ? (
                                        <input
                                            id={id}
                                            type="checkbox"
                                            className="form-checkbox"
                                            checked={!!val}
                                            onChange={(e) => onChange(tname, key, e.target.checked)}
                                        />
                                    ) : typeof val === "number" ? (
                                        <input
                                            id={id}
                                            type="number"
                                            className="form-input"
                                            value={val}
                                            onChange={(e) => onChange(tname, key, Number(e.target.value))}
                                        />
                                    ) : (
                                        <input
                                            id={id}
                                            className="form-input"
                                            value={val ?? ""}
                                            onChange={(e) => onChange(tname, key, e.target.value)}
                                        />
                                    );
                                return (
                                    <div className="row" key={key}>
                                        <label htmlFor={id}>{key}</label>
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
                    {saving ? "保存中…" : "保存"}
                </button>
                <button className="btn ghost" onClick={discard} disabled={!dirty}>放弃更改</button>
            </div>
        </div>
    );
}