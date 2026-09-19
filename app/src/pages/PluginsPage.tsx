import React from "react";
import {useTranslation} from "react-i18next";
import {openUrl} from "@tauri-apps/plugin-opener";
import * as backendIpc from "../lib/ipc";
import type {PacketOperation, PluginInfo, PluginUpdateInfo} from "../lib/ipc";
import {pushToast} from "../lib/toast";
import {usePluginStore} from "../lib/pluginStore";
import PluginMarketplace from "./PluginMarketplace";
import styles from "./PluginsPage.module.css";

type PluginSection = "marketplace" | "installed" | "sources";

export default function PluginsPage() {
    const {t} = useTranslation();
    const [plugins, setPlugins] = React.useState<PluginInfo[]>([]);
    const [permissions, setPermissions] = React.useState<Record<string, PacketOperation[]>>({});
    const [updates, setUpdates] = React.useState<Record<string, PluginUpdateInfo>>({});
    const [busy, setBusy] = React.useState<string | null>(null);
    const [scanErrors, setScanErrors] = React.useState<backendIpc.PluginScanError[]>([]);
    const [uninstallId, setUninstallId] = React.useState<string | null>(null);
    const [removeConfig, setRemoveConfig] = React.useState(false);
    const [selectedId, setSelectedId] = React.useState<string | null>(null);
    const [section, setSection] = React.useState<PluginSection>("marketplace");
    const tabsRef = React.useRef<HTMLDivElement>(null);
    React.useLayoutEffect(() => {
        const tabs = tabsRef.current;
        if (!tabs) return;
        const update = () => {
            const active = tabs.querySelector<HTMLElement>('[aria-selected="true"]');
            if (!active) return;
            tabs.style.setProperty("--tab-left", active.offsetLeft + "px");
            tabs.style.setProperty("--tab-width", active.offsetWidth + "px");
        };
        update();
        const observer = new ResizeObserver(update);
        observer.observe(tabs);
        tabs.querySelectorAll("button").forEach(button => observer.observe(button));
        return () => observer.disconnect();
    }, [section, selectedId]);
    const detailHeading = React.useRef<HTMLHeadingElement>(null);
    React.useEffect(() => {
        if (selectedId) detailHeading.current?.focus();
    }, [selectedId]);
    const sectionContent = React.useRef<HTMLDivElement>(null);
    React.useLayoutEffect(() => {
        sectionContent.current?.getAnimations().forEach(animation => {
            animation.currentTime = 0;
            animation.play();
        });
    }, [section]);
    const fileInput = React.useRef<HTMLInputElement>(null);
    const frontendErrors = usePluginStore((state) => state.errors);

    const accept = React.useCallback((next: PluginInfo[]) => {
        setPlugins(next);
        void backendIpc.getPluginScanErrors().then(setScanErrors).catch((error) => pushToast(String(error), "error"));
        setUpdates((current) => Object.fromEntries(Object.entries(current).filter(([id, update]) =>
            next.some((plugin) => plugin.id === id && plugin.version === update.currentVersion))));
        setPermissions((current) => Object.fromEntries(next.map((plugin) => [
            plugin.id,
            (current[plugin.id] ?? (plugin.approvedPacketPermissions.length
                ? plugin.approvedPacketPermissions
                : plugin.requestedPacketPermissions))
                .filter((operation) => plugin.requestedPacketPermissions.includes(operation)),
        ])));
    }, []);

    React.useEffect(() => {
        void backendIpc.getPlugins().then(async (initial) => {
            accept(initial);
            const checked = await backendIpc.checkPluginUpdates();
            setUpdates(Object.fromEntries(checked.map((update) => [update.pluginId, update])));
        }).catch((error) => pushToast(String(error), "error"));
        const stopStatus = backendIpc.subscribeBackendEvent("plugin_status", accept);
        const stopModules = backendIpc.subscribeBackendEvent("packet_modules", () => void backendIpc.getPlugins().then(accept));
        const stopUpdates = backendIpc.subscribeBackendEvent("plugin_update_available", (update) => {
            setUpdates((current) => ({...current, [update.pluginId]: update}));
        });
        return () => {
            stopStatus();
            stopModules();
            stopUpdates();
        };
    }, [accept]);

    const run = async (key: string, action: () => Promise<PluginInfo[]>) => {
        setBusy(key);
        try {
            accept(await action());
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    };

    const checkUpdates = async () => {
        setBusy("check-updates");
        try {
            const checked = await backendIpc.checkPluginUpdates();
            setUpdates(Object.fromEntries(checked.map((update) => [update.pluginId, update])));
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    };

    const applyUpdate = async (pluginId: string) => {
        await run(`update:${pluginId}`, () => backendIpc.updatePlugin(pluginId));
        await checkUpdates();
    };

    const install = async (file: File) => {
        if (file.size > 128 * 1024 * 1024) {
            pushToast(t("plugins.archive_too_large"), "error");
            return;
        }
        await run("install", async () => {
            const archive = await new Promise<string>((resolve, reject) => {
                const reader = new FileReader();
                reader.onload = () => resolve(String(reader.result).split(",", 2)[1]);
                reader.onerror = () => reject(reader.error);
                reader.onabort = () => reject(new Error(t("plugins.install_cancelled")));
                reader.readAsDataURL(file);
            });
            const next = await backendIpc.installPlugin(archive);
            pushToast(t("plugins.installed"), "success");
            return next;
        });
    };

    const togglePermission = (pluginId: string, operation: PacketOperation) => {
        setPermissions((current) => {
            const selected = current[pluginId] ?? [];
            return {
                ...current,
                [pluginId]: selected.includes(operation)
                    ? selected.filter((item) => item !== operation)
                    : [...selected, operation],
            };
        });
    };

    return (
        <div className={styles.page}>
            <header className={styles.header}>
                <div>
                    <h1 ref={detailHeading} tabIndex={-1}>{t(selectedId ? "plugins.detail_title" : "plugins.title")}</h1>
                    {selectedId ? <button className={styles.backButton} onClick={() => {
                        setSelectedId(null);
                        setUninstallId(null);
                    }}><span className="ms" aria-hidden="true">arrow_back</span>{t("plugins.back_to_list")}</button>
                        : null}
                </div>
                <div className={styles.actions} hidden={selectedId !== null || section !== "installed"}>
                    <input ref={fileInput} type="file" accept=".zip,application/zip" hidden onChange={(event) => {
                        const file = event.target.files?.[0];
                        event.target.value = "";
                        if (file) void install(file);
                    }}/>
                    <button className="nav-btn" disabled={busy !== null} onClick={() => fileInput.current?.click()}>
                        <span className="ms" aria-hidden="true">add</span>
                        {t("plugins.install")}
                    </button>
                    <button className="btn ghost" disabled={busy !== null} onClick={() => void checkUpdates()}>
                        <span className="ms" aria-hidden="true">system_update</span>
                        {t("plugins.check_updates")}
                    </button>
                    <button className="btn ghost" onClick={() => void backendIpc.openPluginDir().catch((error) => pushToast(String(error), "error"))}>
                        <span className="ms" aria-hidden="true">folder_open</span>
                        {t("plugins.open_folder")}
                    </button>
                    <button className="nav-btn" disabled={busy !== null} onClick={() => void run("rescan", backendIpc.rescanPlugins)}>
                        <span className="ms" aria-hidden="true">refresh</span>
                        {t("plugins.rescan")}
                    </button>
                </div>
            </header>

            {!selectedId ? <div className={styles.tabs} ref={tabsRef} role="tablist" aria-label={t("plugins.sections_label")}>
                <span className={styles.tabIndicator} aria-hidden="true"/>
                {(["marketplace", "installed", "sources"] as const).map((item) => <button
                    key={item}
                    type="button"
                    role="tab"
                    aria-selected={section === item}
                    className={section === item ? styles.activeTab : undefined}
                    onClick={() => setSection(item)}
                >{t(`plugins.sections.${item}`)}</button>)}
            </div> : null}

            <div ref={sectionContent} className="plugin-section-content">
            {section !== "installed" ? <PluginMarketplace
                section={section}
                installed={plugins}
                onInstalled={(next, id) => {
                    accept(next);
                    setSection("installed");
                    setSelectedId(id);
                }}
            /> : <>
            {scanErrors.length > 0 ? <section className={styles.scanErrors} role="alert">
                <h2>{t("plugins.scan_errors", {count: scanErrors.length})}</h2>
                {scanErrors.map((failure) => <div key={failure.path}>
                    <code>{failure.path}</code>
                    <p>{failure.error}</p>
                </div>)}
            </section> : null}

            {(selectedId ? !plugins.some((plugin) => plugin.id === selectedId) : plugins.length === 0) ? (
                <div className={styles.empty}>
                    <span className="ms" aria-hidden="true">extension_off</span>
                    <strong>{t(selectedId ? "plugins.detail_unavailable" : "plugins.empty_title")}</strong>
                    {!selectedId ? <p>{t("plugins.empty_body")}</p> : null}
                </div>
            ) : (
                <div className={selectedId ? styles.detailPage : styles.list}>
                    {plugins.filter((plugin) => !selectedId || plugin.id === selectedId).map((plugin) => {
                        const selected = permissions[plugin.id] ?? [];
                        const update = updates[plugin.id];
                        const state = frontendErrors[plugin.id] || plugin.lastError ? "error" : plugin.running ? "running" : plugin.enabled ? "error" : "disabled";
                        if (!selectedId) return (
                            <button type="button" className={styles.summaryCard} key={plugin.id}
                                onClick={() => setSelectedId(plugin.id)}>
                                <span className={styles.summaryIcon}><span className="ms" aria-hidden="true">extension</span></span>
                                <span className={styles.summaryContent}>
                                    <span className={styles.summaryTitle}>{plugin.name}</span>
                                    <span className={styles.summaryDescription}>{plugin.description || plugin.id}</span>
                                    <span className={styles.summaryMeta}>
                                        <span>{t("plugins.version", {version: plugin.version})}</span>
                                        {update?.available ? <span className={styles.updateBadge}>{t("plugins.update_available", {version: update.latestVersion})}</span> : null}
                                    </span>
                                </span>
                                <span className={styles.summaryEnd}>
                                    <span className={styles.status} data-state={state}>{t(`plugins.status.${state}`)}</span>
                                    <span className={styles.summaryLink}>{t("plugins.view_details")}<span className="ms" aria-hidden="true">chevron_right</span></span>
                                </span>
                            </button>
                        );
                        return (
                            <article className={styles.card} key={plugin.id}>
                                <div className={styles.overview}>
                                <div className={styles.cardHeader}>
                                    <div>
                                        <h2>{plugin.name}</h2>
                                        <code>{plugin.id}</code>
                                    </div>
                                    <span className={styles.status} data-state={state}>{t(`plugins.status.${state}`)}</span>
                                </div>
                                <div className={styles.meta}>
                                    <span>{t("plugins.version", {version: plugin.version})}</span>
                                    <span>{t("plugins.api_version", {version: plugin.apiVersion})}</span>
                                    {plugin.author ? <span>{t("plugins.author", {author: plugin.author})}</span> : null}
                                    {plugin.hasFrontend ? <span>{t("plugins.frontend")}</span> : null}
                                </div>
                                {plugin.description ? <p className={styles.description}>{plugin.description}</p> : null}
                                {plugin.homepage || plugin.source || plugin.issues ? (
                                    <div className={styles.links}>
                                        {plugin.homepage ? <button type="button" title={t("plugins.homepage")} aria-label={t("plugins.homepage")} onClick={() => void openUrl(plugin.homepage!)}><span className="ms" aria-hidden="true">home</span></button> : null}
                                        {plugin.source ? <button type="button" title={t("plugins.source")} aria-label={t("plugins.source")} onClick={() => void openUrl(plugin.source!)}><span className="ms" aria-hidden="true">code</span></button> : null}
                                        {plugin.issues ? <button type="button" title={t("plugins.issues")} aria-label={t("plugins.issues")} onClick={() => void openUrl(plugin.issues!)}><span className="ms" aria-hidden="true">bug_report</span></button> : null}
                                    </div>
                                ) : null}

                                </div>
                                <div className={styles.details}>
                                {plugin.hasUpdateSource ? (
                                    <section className={styles.updateSection}>
                                        <div className={styles.updateHeader}>
                                            <h3>{t("plugins.updates")}</h3>
                                            <label>
                                                <input
                                                    type="checkbox"
                                                    checked={plugin.autoUpdate}
                                                    disabled={busy !== null}
                                                    onChange={(event) => void run(`auto-update:${plugin.id}`, () => backendIpc.setPluginAutoUpdate(plugin.id, event.target.checked))}
                                                />
                                                {t("plugins.auto_update")}
                                            </label>
                                        </div>
                                        {update?.error ? <p className={styles.updateError}>{t("plugins.update_check_failed", {error: update.error})}</p>
                                            : update?.available ? (
                                                <div className={styles.updateAvailable}>
                                                    <span>{t("plugins.update_available", {version: update.latestVersion})}</span>
                                                    <button className="nav-btn" disabled={busy !== null} onClick={() => void applyUpdate(plugin.id)}>{t("plugins.update_now")}</button>
                                                </div>
                                            ) : <p className={styles.hint}>{update
                                                ? update.latestVersion ? t("plugins.up_to_date") : t("plugins.no_update_reported")
                                                : t("plugins.checking_update")}</p>}
                                        {update?.releaseNotes ? <p className={styles.releaseNotes}>{update.releaseNotes}</p> : null}
                                    </section>
                                ) : null}

                                <section className={styles.permissionSection}>
                                    <h3>{t("plugins.permissions")}</h3>
                                    <div className={styles.permissions}>
                                        {plugin.requestedPacketPermissions.map((operation) => (
                                            <label key={operation}>
                                                <input
                                                    type="checkbox"
                                                    checked={selected.includes(operation)}
                                                    disabled={plugin.enabled || busy !== null}
                                                    onChange={() => togglePermission(plugin.id, operation)}
                                                />
                                                {t(`packet_pipeline.operations.${operation}`)}
                                            </label>
                                        ))}
                                        {plugin.requestedPacketPermissions.length === 0 ? <span>{t("plugins.no_permissions")}</span> : null}
                                    </div>
                                </section>

                                <section className={styles.moduleSection}>
                                    <h3>{t("plugins.modules", {count: plugin.modules.length})}</h3>
                                    {plugin.modules.length ? (
                                        <div className={styles.modules}>
                                            {plugin.modules.map((module) => (
                                                <div key={module.id}>
                                                    <span className={module.online ? styles.dotOnline : styles.dotOffline}/>
                                                    <strong>{module.name}</strong>
                                                    <code>{module.id}</code>
                                                </div>
                                            ))}
                                        </div>
                                    ) : <p className={styles.hint}>{t("plugins.no_modules")}</p>}
                                </section>

                                </div>
                                {plugin.lastError ? <pre className={styles.error}>{plugin.lastError}</pre> : null}
                                {frontendErrors[plugin.id] ? <pre className={styles.error}>{frontendErrors[plugin.id]}</pre> : null}

                                <div className={styles.cardActions}>
                                    <button className={`${styles.disableButton} ${styles.uninstallButton}`} disabled={busy !== null} onClick={() => {
                                        setUninstallId(plugin.id);
                                        setRemoveConfig(false);
                                    }}>{t("plugins.uninstall")}</button>
                                    {plugin.enabled ? (
                                        <>
                                            <button className="btn ghost" disabled={busy !== null} onClick={() => void run(plugin.id, () => backendIpc.restartPlugin(plugin.id))}>
                                                {t("plugins.restart")}
                                            </button>
                                            <button className={styles.disableButton} disabled={busy !== null} onClick={() => void run(plugin.id, () => backendIpc.setPluginEnabled(plugin.id, false, selected))}>
                                                {t("plugins.disable")}
                                            </button>
                                        </>
                                    ) : (
                                        <button className="nav-btn" disabled={busy !== null} onClick={() => void run(plugin.id, () => backendIpc.setPluginEnabled(plugin.id, true, selected))}>
                                            {t("plugins.approve_enable")}
                                        </button>
                                    )}
                                </div>
                                {uninstallId === plugin.id ? <div className={styles.uninstallConfirm}>
                                    <strong>{t("plugins.uninstall_confirm", {name: plugin.name})}</strong>
                                    <p>{t("plugins.uninstall_hint")}</p>
                                    <label><input type="checkbox" checked={removeConfig} disabled={busy !== null} onChange={(event) => setRemoveConfig(event.target.checked)}/>{t("plugins.remove_config")}</label>
                                    <div className={styles.actions}>
                                        <button className="btn ghost" disabled={busy !== null} onClick={() => setUninstallId(null)}>{t("plugins.cancel")}</button>
                                        <button className={styles.disableButton} disabled={busy !== null} onClick={() => void run(`uninstall:${plugin.id}`, async () => {
                                            const next = await backendIpc.uninstallPlugin(plugin.id, removeConfig);
                                            setUninstallId(null);
                                            setSelectedId(null);
                                            return next;
                                        })}>{t("plugins.confirm_uninstall")}</button>
                                    </div>
                                </div> : null}
                            </article>
                        );
                    })}
                </div>
            )}
            </>}
            </div>
        </div>
    );
}
