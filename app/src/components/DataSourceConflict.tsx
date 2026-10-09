import React from "react";
import {useTranslation} from "react-i18next";
import * as ipc from "../lib/ipc";
import {pushToast} from "../lib/toast";
import styles from "./DataSourceConflict.module.css";
import "../fonts/material-symbols.css";

export default function DataSourceConflict() {
    const {t} = useTranslation();
    const dialog = React.useRef<HTMLDialogElement>(null);
    const [status, setStatus] = React.useState<ipc.DataSourceStatus | null>(null);
    const [busy, setBusy] = React.useState(false);
    const [error, setError] = React.useState(false);
    const [closing, setClosing] = React.useState(false);
    const open = Boolean(status?.pending);

    React.useEffect(() => {
        let mounted = true;
        const update = (next: ipc.DataSourceStatus) => {
            if (mounted) setStatus(previous => !previous || next.revision >= previous.revision ? next : previous);
        };
        let unlisten: (() => void) | undefined;
        const refresh = () => { void ipc.getDataSources().then(update).catch(() => {}); };
        void ipc.onBackendEvent("data_source_status", update).then(stop => {
            if (!mounted) { stop(); return; }
            unlisten = stop;
            refresh();
        }).catch(refresh);
        const stopResync = ipc.subscribeBackendEvent("resync_required", refresh);
        return () => { mounted = false; unlisten?.(); stopResync(); };
    }, []);

    React.useEffect(() => {
        const element = dialog.current;
        if (!element) return;
        if (open) {
            setClosing(false);
            setError(false);
            if (!element.open) element.showModal();
        } else if (element.open) {
            setClosing(true);
            const timer = window.setTimeout(() => { element.close(); setClosing(false); }, 180);
            return () => window.clearTimeout(timer);
        }
    }, [open]);

    const choose = async (source: ipc.DataSource) => {
        if (busy) return;
        setBusy(true);
        setError(false);
        try {
            if (status?.pending) {
                const next = await ipc.selectDataSource(source, status.revision);
                setStatus(previous => !previous || next.revision >= previous.revision ? next : previous);
                pushToast(t("data_source.selected", {source: t(`data_source.${source}`)}), "success");
            }
        } catch {
            setError(true);
            void ipc.getDataSources().then(next => setStatus(previous => !previous || next.revision >= previous.revision ? next : previous)).catch(() => {});
        } finally { setBusy(false); }
    };

    return <dialog ref={dialog} className={`${styles.screen} ${closing ? styles.closing : ""}`}
        aria-labelledby="source-conflict-title" aria-describedby="source-conflict-description" aria-busy={busy}
        onCancel={event => event.preventDefault()}>
        <div className={styles.content}>
            <div className={styles.signal} aria-hidden="true">
                <span className={`ms ${styles.endpoint}`}>cloud</span><span className={styles.line}/>
                <img className={styles.logo} src="/logo.svg" alt=""/>
                <span className={styles.line}/><span className={`ms ${styles.endpoint}`}>computer</span>
            </div>
            <h1 id="source-conflict-title">{t("data_source.title")}</h1>
            <p id="source-conflict-description" className={styles.description}>{t("data_source.description")}</p>
            <div className={styles.choices}>
                {(["packet", "qyzz"] as const).map(source => <button key={source}
                    className={styles.choice} disabled={busy || closing} onClick={() => void choose(source)}>
                    <span className={`ms ${styles.choiceIcon}`} aria-hidden="true">{source === "packet" ? "cloud" : "computer"}</span>
                    <strong>{t(`data_source.${source}`)}</strong>
                </button>)}
            </div>
            {busy && <p role="status">{t("data_source.switching")}</p>}
            {error && <p className={styles.error} role="alert">{t("data_source.failed")}</p>}
        </div>
    </dialog>;
}
