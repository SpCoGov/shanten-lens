import React from "react";
import {useTranslation} from "react-i18next";
import Modal from "../components/Modal";
import {useAutoRunner} from "../lib/autoRunnerStore";
import {subscribeBackendEvent} from "../lib/ipc";
import * as direct from "../lib/directConnection";
import styles from "./DirectConnectionPage.module.css";

const emptyAccount: direct.DirectAccountInput = {label: "", region: "cn", login_method: "password", username: "", uid: "",
    oauth_type: 21, client_version_string: "", route: null};
const regions = ["cn", "jp", "en", "kr"] as const;

export default function DirectConnectionPage() {
    const {t, i18n} = useTranslation();
    const {status: autorun} = useAutoRunner();
    const [accounts, setAccounts] = React.useState<direct.DirectAccount[]>([]);
    const [status, setStatus] = React.useState<direct.DirectStatus | null>(null);
    const [selectedId, setSelectedId] = React.useState("");
    const [editing, setEditing] = React.useState<direct.DirectAccountInput | null>(null);
    const [deleting, setDeleting] = React.useState<string | null>(null);
    const [busy, setBusy] = React.useState(false);
    const [error, setError] = React.useState("");
    const [routes, setRoutes] = React.useState<direct.DirectRoute[]>([]);
    const [routesLoading, setRoutesLoading] = React.useState(false);
    const [routesFailed, setRoutesFailed] = React.useState(false);
    const [routeReload, setRouteReload] = React.useState(0);
    const [captureGuide, setCaptureGuide] = React.useState<string | null>(null);
    const [captureResult, setCaptureResult] = React.useState<{account_id: string; uid: string} | null>(null);
    const [emailCodeSent, setEmailCodeSent] = React.useState(false);
    const [emailCooldown, setEmailCooldown] = React.useState(0);
    const selected = accounts.find(account => account.id === selectedId) ?? accounts[0];
    const deletingAccount = accounts.find(account => account.id === deleting);
    const saved = accounts.find(account => account.id === editing?.id);
    const sameIdentity = saved && editing && saved.region === editing.region && saved.login_method === editing.login_method;
    const keepPassword = sameIdentity && saved.has_password && saved.username === editing.username.trim();
    const keepCode = sameIdentity && saved.has_code && saved.uid === editing.uid.trim();
    const keepEmail = sameIdentity && saved.has_session && saved.username === editing.username.trim();
    const active = status?.state === "connected" || status?.state === "connecting";
    const capturing = Boolean(status?.capture_account_id);
    const captureAccount = accounts.find(account => account.id === (captureGuide ?? status?.capture_account_id));
    const captured = Boolean(captureGuide && captureResult?.account_id === captureGuide);
    const statusRef = React.useRef(status);
    statusRef.current = status;
    const errorText = (reason: string) => t("direct_connection.error", {reason: i18n.exists(reason) ? t(reason) : reason});
    const ready = (account: direct.DirectAccount) => account.login_method === "password" ? account.has_password && Boolean(account.username)
        : account.login_method === "email" ? account.has_session : account.has_code && Boolean(account.uid);

    React.useEffect(() => {
        if (!emailCooldown) return;
        const timer = window.setTimeout(() => setEmailCooldown(value => Math.max(0, value - 1)), 1000);
        return () => window.clearTimeout(timer);
    }, [emailCooldown]);
    React.useEffect(() => { setEmailCodeSent(false); setEmailCooldown(0); }, [editing?.region, editing?.username, editing?.login_method]);
    React.useEffect(() => {
        let mounted = true;
        let revision = 0;
        const update = (next: direct.DirectStatus) => {
            if (!mounted) return;
            setStatus(next);
            setSelectedId(value => value || next.account_id || "");
        };
        const refresh = () => {
            const request = ++revision;
            void Promise.all([direct.getDirectAccounts(), direct.getDirectStatus()]).then(([next, connection]) => {
                if (mounted && request === revision) {setAccounts(next); update(connection);}
            }).catch(reason => { if (mounted) setError(String(reason)); });
        };
        const unlisten = subscribeBackendEvent("direct_connection_status", update);
        const stopAccounts = subscribeBackendEvent("direct_accounts_changed", refresh);
        const stopCaptured = subscribeBackendEvent("direct_account_captured", result => {setCaptureResult(result); refresh();});
        const stopResync = subscribeBackendEvent("resync_required", refresh);
        const timer = window.setInterval(() => {if (statusRef.current?.capture_account_id) refresh();}, 5000);
        refresh();
        return () => { mounted = false; unlisten(); stopAccounts(); stopCaptured(); stopResync(); window.clearInterval(timer); };
    }, []);

    const region = editing?.region;
    React.useEffect(() => {
        if (!region) return;
        let mounted = true;
        setRoutes([]); setRoutesLoading(true); setRoutesFailed(false);
        void direct.getDirectRoutes(region).then(next => { if (mounted) setRoutes(next); })
            .catch(() => { if (mounted) setRoutesFailed(true); })
            .finally(() => { if (mounted) setRoutesLoading(false); });
        return () => { mounted = false; };
    }, [region, routeReload]);

    async function run(action: () => Promise<void>) {
        setBusy(true); setError("");
        try { await action(); } catch (reason) { setError(String(reason)); } finally { setBusy(false); }
    }
    async function startCapture(id: string) {
        setCaptureResult(null); setStatus(await direct.captureDirectAccount(id)); setCaptureGuide(id);
    }
    const save = (event: React.FormEvent) => {
        event.preventDefault();
        if (!editing) return;
        void run(async () => {
            const next = await direct.saveDirectAccount(editing);
            const account = next.find(account => account.id === editing.id) ?? next.find(account => !accounts.some(previous => previous.id === account.id));
            setAccounts(next); setSelectedId(account?.id ?? ""); setEditing(null); setCaptureResult(null);
            if (account?.login_method === "oauth" && !account.has_code) await startCapture(account.id);
        });
    };
    const editAccount = (account: direct.DirectAccount) => {
        const {has_password: _password, has_code: _code, has_session: _session, ...fields} = account;
        setEditing(fields); setError("");
    };
    const cancelCapture = () => run(async () => {setStatus(await direct.captureDirectAccount(null)); setCaptureGuide(null);});
    const displayed = active ? accounts.find(account => account.id === status?.account_id) : selected;

    return <div className={styles.page}>
        <section className={styles.panel}>
            <div className={styles.heading}>
                <div className={styles.connectionTitle}>
                    <span className={`${styles.statusIcon} ${status?.state === "connected" ? styles.success : ""}`}><span className="ms" aria-hidden="true">{active ? "lan" : "link_off"}</span></span>
                    <div><h3>{t("direct_connection.source_title")}</h3><span className={styles.muted} role="status">{t(`direct_connection.state_${status?.state ?? "loading"}`)}</span></div>
                </div>
                <span className={styles.tag}>{t(active ? "direct_connection.source_direct" : "direct_connection.source_mitm")}</span>
            </div>
            <div className={styles.connectBar}>
                <div className={styles.connectionAccount}>
                    <span className={styles.eyebrow}>{t("direct_connection.account")}</span>
                    <strong>{displayed?.label ?? t("direct_connection.no_accounts")}</strong>
                    {displayed && <span className={styles.muted}>{t(`direct_connection.region_${displayed.region}`)} · {t(`direct_connection.login_${displayed.login_method}`)}</span>}
                </div>
                <button className={`nav-btn ${styles.primary}`} disabled={busy || !status || (!active && (!selected || !ready(selected) || capturing)) || autorun.running}
                    onClick={() => void run(async () => {
                        if (active) setStatus(await direct.disconnectDirect());
                        else if (selected) setStatus(await direct.connectDirect(selected.id));
                    })}>
                    <span className="ms" aria-hidden="true">{active ? "link_off" : "link"}</span>
                    {t(active ? (status?.state === "connecting" ? "direct_connection.cancel_connect" : "direct_connection.disconnect") : "direct_connection.connect")}
                </button>
            </div>
            {active && <div className={styles.connectionMeta}>
                {status?.route && <span>{t("direct_connection.current_route", {route: status.route})}</span>}
                {status?.last_heartbeat_ms && <span>{t("direct_connection.heartbeat", {time: new Date(status.last_heartbeat_ms).toLocaleTimeString(i18n.language)})}</span>}
            </div>}
            {status?.error && <p className={styles.error} role="alert">{errorText(status.error)}</p>}
            {autorun.running && <p className={styles.muted}>{t("direct_connection.stop_before_switch")}</p>}
        </section>

        <section className={styles.panel} aria-labelledby="direct-accounts-heading">
            <div className={styles.heading}>
                <h3 id="direct-accounts-heading">{t("direct_connection.accounts_title")} <span className={styles.count}>{accounts.length}</span></h3>
                <button className="nav-btn" disabled={busy} onClick={() => {setEditing({...emptyAccount}); setError("");}}>
                    <span className="ms" aria-hidden="true">add</span>{t("direct_connection.add")}</button>
            </div>
            {capturing && <div className={styles.notice} role="status">
                <span className="ms" aria-hidden="true">sensors</span><span>{t("direct_connection.capture_waiting", {name: accounts.find(account => account.id === status?.capture_account_id)?.label ?? ""})}</span>
                <button className="nav-btn" disabled={busy} onClick={() => void cancelCapture()}>{t("direct_connection.capture_cancel")}</button>
            </div>}
            {captureResult && !capturing && <p className={styles.successText} role="status">{t("direct_connection.capture_updated", {uid: captureResult.uid})}</p>}
            {accounts.length === 0 ? <div className={styles.empty}><span className="ms" aria-hidden="true">person_add</span><p>{t("direct_connection.no_accounts")}</p></div> :
                <div className={styles.accountList} role="group" aria-label={t("direct_connection.account")}>
                    {accounts.map(account => {
                        const inUse = active && status?.account_id === account.id;
                        const listening = status?.capture_account_id === account.id;
                        return <article className={styles.accountCard} key={account.id} data-selected={selected?.id === account.id}>
                            <label className={styles.accountChoice}>
                                <input type="radio" name="direct-account" checked={selected?.id === account.id} disabled={busy || active || autorun.running}
                                    onChange={() => setSelectedId(account.id)} aria-label={t("direct_connection.select_account", {name: account.label})}/>
                                <span className={styles.accountDetails}>
                                    <span className={styles.accountName}><strong>{account.label}</strong><span className={styles.tag}>{t(`direct_connection.region_${account.region}`)}</span>
                                        {inUse && <span className={styles.successText}>{t(`direct_connection.state_${status?.state}`)}</span>}
                                        {listening && <span className={styles.muted}>{t("direct_connection.listening")}</span>}</span>
                                    <span className={styles.identity}>{account.login_method === "oauth" ? account.uid ? t("direct_connection.uid_value", {uid: account.uid}) : t("direct_connection.awaiting_capture") : account.username}</span>
                                    <span className={styles.accountMeta}>{t(`direct_connection.login_${account.login_method}`)}<span>·</span>{account.route ? t("direct_connection.route_name", {id: account.route}) : t("direct_connection.route_auto")}
                                        {!ready(account) && <span className={styles.warning}>{t("direct_connection.awaiting_capture")}</span>}</span>
                                </span>
                            </label>
                            <div className={styles.accountActions}>
                                {account.login_method === "oauth" && <button className="nav-btn" disabled={busy || active || capturing || autorun.running}
                                    onClick={() => void run(() => startCapture(account.id))}><span className="ms" aria-hidden="true">sensors</span>{t("direct_connection.capture")}</button>}
                                <button className="nav-btn" disabled={busy || inUse || listening} onClick={() => editAccount(account)} aria-label={t("direct_connection.edit_named", {name: account.label})}>
                                    <span className="ms" aria-hidden="true">edit</span>{t("direct_connection.edit_short")}</button>
                                <button className={`nav-btn ${styles.danger}`} disabled={busy || inUse} onClick={() => {setDeleting(account.id); setError("");}} aria-label={t("direct_connection.delete_named", {name: account.label})}>
                                    <span className="ms" aria-hidden="true">delete</span>{t("direct_connection.delete_short")}</button>
                            </div>
                        </article>;
                    })}
                </div>}
        </section>
        {error && !editing && !deleting && !captureGuide && <p className={styles.error} role="alert">{errorText(error)}</p>}

        <Modal open={Boolean(editing)} onClose={() => {if (!busy) setEditing(null);}}
            title={t(editing?.id ? "direct_connection.edit" : "direct_connection.add")} width={680} className={styles.dialog}>
            {editing && <form onSubmit={save}>
                <fieldset className={styles.form} disabled={busy}>
                    <div className={styles.formBody}>
                        <label className={styles.field}>{t("direct_connection.name")}<input className="form-input" required maxLength={80} autoFocus
                            value={editing.label} onChange={event => setEditing({...editing, label: event.target.value})}/></label>
                        <fieldset className={styles.choiceGroup}><legend>{t("direct_connection.region")}</legend><div className={styles.regionChoices}>
                            {regions.map(value => <label className={styles.choice} key={value}>
                                <input type="radio" name="direct-region" checked={editing.region === value} onChange={() => setEditing({...editing, region: value, route: null,
                                    oauth_type: value === "cn" || value === "jp" ? 21 : 22, client_version_string: "",
                                    login_method: value === "cn" ? "password" : "email", password: undefined, code: undefined, verification_code: undefined})}/>
                                <span>{t(`direct_connection.region_${value}`)}</span></label>)}
                        </div></fieldset>
                        <fieldset className={styles.choiceGroup}><legend>{t("direct_connection.login_method")}</legend><div className={styles.methodChoices}>
                            {([editing.region === "cn" ? "password" : "email", "oauth"] as const).map(method => <label className={styles.choice} key={method}>
                                <input type="radio" name="direct-method" checked={editing.login_method === method} onChange={() => setEditing({...editing, login_method: method, password: undefined, code: undefined, verification_code: undefined})}/>
                                <span className="ms" aria-hidden="true">{method === "password" ? "key" : method === "email" ? "mail" : "sensors"}</span><span>{t(`direct_connection.login_${method}`)}</span></label>)}
                        </div></fieldset>
                        <div className={styles.credentials}>
                            {editing.login_method === "password" ? <div className={styles.fields}>
                                <label className={styles.field}>{t("direct_connection.username")}<input className="form-input" autoComplete="username" required
                                    value={editing.username} onChange={event => setEditing({...editing, username: event.target.value})}/></label>
                                <label className={styles.field}>{t("direct_connection.password")}<input className="form-input" type="password" autoComplete="new-password"
                                    required={!keepPassword} placeholder={keepPassword ? t("direct_connection.keep_secret") : undefined}
                                    value={editing.password ?? ""} onChange={event => setEditing({...editing, password: event.target.value})}/></label>
                            </div> : editing.login_method === "email" ? <>
                                <div className={styles.emailRow}>
                                    <label className={styles.field}>{t("direct_connection.email")}<input className="form-input" type="email" autoComplete="email" required
                                        value={editing.username} onChange={event => setEditing({...editing, username: event.target.value, verification_code: undefined})}/></label>
                                    <button type="button" className="nav-btn" disabled={emailCooldown > 0 || !editing.username.trim()}
                                        onClick={() => void run(async () => {
                                            await direct.sendDirectEmailCode(editing.region, editing.username); setEmailCodeSent(true); setEmailCooldown(60);
                                        })}>{emailCooldown ? t("direct_connection.resend_code", {seconds: emailCooldown}) : t("direct_connection.send_code")}</button>
                                </div>
                                <label className={styles.field}>{t("direct_connection.verification_code")}<input className="form-input" inputMode="numeric" autoComplete="one-time-code"
                                    required={!keepEmail} pattern="[0-9]{6}" maxLength={6} value={editing.verification_code ?? ""}
                                    placeholder={keepEmail ? t("direct_connection.keep_email_session") : undefined}
                                    onChange={event => setEditing({...editing, verification_code: event.target.value})}/></label>
                                {emailCodeSent && <p className={styles.successText} role="status">{t("direct_connection.email_code_sent")}</p>}
                            </> : <>
                                <div className={styles.fields}>
                                    <label className={styles.field}>{t("direct_connection.uid")}<input className="form-input" autoComplete="off"
                                        value={editing.uid} onChange={event => setEditing({...editing, uid: event.target.value})}/></label>
                                    <label className={styles.field}>{t("direct_connection.code")}<input className="form-input" type="password" autoComplete="off"
                                        placeholder={keepCode ? t("direct_connection.keep_secret") : undefined}
                                        value={editing.code ?? ""} onChange={event => setEditing({...editing, code: event.target.value})}/></label>
                                </div>{!keepCode && <p className={styles.muted}>{t("direct_connection.oauth_hint")}</p>}
                            </>}
                        </div>
                        <fieldset className={styles.choiceGroup}><legend>{t("direct_connection.route")}</legend>
                            <div className={styles.routeHeading}><span className={styles.muted}>{t("direct_connection.route_latency")}</span>
                                <button type="button" className="nav-btn" disabled={routesLoading} onClick={() => setRouteReload(value => value + 1)}>
                                    <span className="ms" aria-hidden="true">refresh</span>{t(routesLoading ? "direct_connection.routes_loading" : "direct_connection.routes_refresh")}</button></div>
                            <div className={styles.routeChoices} aria-busy={routesLoading}>
                                <label className={`${styles.choice} ${styles.autoRoute}`}><input type="radio" name="direct-route" checked={!editing.route} onChange={() => setEditing({...editing, route: null})}/>
                                    <span className="ms" aria-hidden="true">network_check</span><span>{t("direct_connection.route_auto")}</span><small>{t("direct_connection.route_auto_short")}</small></label>
                                {editing.route && !routes.some(route => route.id === editing.route) && <label className={styles.choice}>
                                    <input type="radio" name="direct-route" checked readOnly/><span>{t("direct_connection.route_name", {id: editing.route})}</span><small>{t(routesLoading ? "direct_connection.routes_loading" : "direct_connection.route_missing")}</small></label>}
                                {routes.map(route => <label className={styles.choice} key={route.id}>
                                    <input type="radio" name="direct-route" checked={editing.route === route.id} onChange={() => setEditing({...editing, route: route.id})}/>
                                    <span>{t("direct_connection.route_name", {id: route.id})}</span><span className={`${styles.latency} ${route.latency_ms === null ? styles.warning : styles.successText}`}>
                                        {route.latency_ms === null ? t("direct_connection.route_unreachable") : t("direct_connection.latency", {ms: route.latency_ms})}</span></label>)}
                            </div>
                            {routesFailed && <p className={styles.warning} role="status">{t("direct_connection.routes_failed")}</p>}
                        </fieldset>
                        {error && <p className={styles.error} role="alert">{errorText(error)}</p>}
                    </div>
                    <div className={styles.footer}><button className="nav-btn" type="button" onClick={() => setEditing(null)}>{t("common.cancel")}</button>
                        <button className={`nav-btn ${styles.primary}`} type="submit"><span className="ms" aria-hidden="true">{busy ? "hourglass_top" : "check"}</span>{t(editing.login_method === "oauth" && !editing.code?.trim() && !keepCode ? "direct_connection.save_and_listen" : "common.save")}</button></div>
                </fieldset>
            </form>}
        </Modal>

        <Modal open={Boolean(deletingAccount)} onClose={() => {if (!busy) setDeleting(null);}} title={t("direct_connection.delete")} width={440} className={styles.dialog}>
            <div className={styles.messageBody}><span className={`${styles.messageIcon} ${styles.danger}`}><span className="ms" aria-hidden="true">person_remove</span></span>
                <p>{t("direct_connection.delete_confirm", {name: deletingAccount?.label})}</p>
                {error && <p className={styles.error} role="alert">{errorText(error)}</p>}</div>
            <div className={styles.footer}><button className="nav-btn" disabled={busy} onClick={() => setDeleting(null)}>{t("common.cancel")}</button>
                <button className={`nav-btn ${styles.danger}`} disabled={busy} onClick={() => void run(async () => {
                    if (!deletingAccount) return;
                    setAccounts(await direct.deleteDirectAccount(deletingAccount.id)); setStatus(await direct.getDirectStatus());
                    if (captureResult?.account_id === deletingAccount.id) setCaptureResult(null);
                    if (captureGuide === deletingAccount.id) setCaptureGuide(null);
                    setDeleting(null);
                })}>{t("direct_connection.delete")}</button></div>
        </Modal>

        <Modal open={Boolean(captureGuide && captureAccount)} onClose={() => setCaptureGuide(null)}
            title={t(captured ? "direct_connection.capture_success" : "direct_connection.capture_started")} width={480} className={styles.dialog}>
            <div className={styles.messageBody}>
                <span className={`${styles.messageIcon} ${captured ? styles.success : ""}`}><span className="ms" aria-hidden="true">{captured ? "check_circle" : "sensors"}</span></span>
                <strong>{captureAccount?.label} · {t(`direct_connection.region_${captureAccount?.region ?? "cn"}`)}</strong>
                <p role="status">{captured ? t("direct_connection.capture_updated", {uid: captureResult?.uid}) : capturing ?
                    t("direct_connection.capture_relogin", {region: t(`direct_connection.region_${captureAccount?.region ?? "cn"}`)}) : t("direct_connection.capture_ended")}</p>
                {error && <p className={styles.error} role="alert">{errorText(error)}</p>}
            </div>
            <div className={styles.footer}>
                {capturing && <button className="nav-btn" disabled={busy} onClick={() => void cancelCapture()}>{t("direct_connection.capture_cancel")}</button>}
                <button className={`nav-btn ${styles.primary}`} onClick={() => setCaptureGuide(null)}>{t(capturing ? "direct_connection.capture_background" : "direct_connection.capture_understood")}</button>
            </div>
        </Modal>
    </div>;
}
