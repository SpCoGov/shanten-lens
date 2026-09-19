import React from "react";
import {useTranslation} from "react-i18next";
import {openUrl} from "@tauri-apps/plugin-opener";
import * as backendIpc from "../lib/ipc";
import type {MarketplaceInstallBlock, MarketplacePluginInfo, MarketplaceSnapshot, PluginInfo} from "../lib/ipc";
import {pushToast} from "../lib/toast";
import styles from "./PluginMarketplace.module.css";

type Props = {
    section: "marketplace" | "sources";
    installed: PluginInfo[];
    onInstalled: (plugins: PluginInfo[], id: string) => void;
};

function blockKey(block: MarketplaceInstallBlock) {
    return `plugins.marketplace.block.${block}` as const;
}

export default function PluginMarketplace({section, installed, onInstalled}: Props) {
    const {t} = useTranslation();
    const [snapshot, setSnapshot] = React.useState<MarketplaceSnapshot | null>(null);
    const [busy, setBusy] = React.useState<string | null>("refresh");
    const [query, setQuery] = React.useState("");
    const [sourceFilter, setSourceFilter] = React.useState("");
    const [categoryFilter, setCategoryFilter] = React.useState("");
    const [sourceUrl, setSourceUrl] = React.useState("");

    const refresh = React.useCallback(async () => {
        setBusy("refresh");
        try {
            setSnapshot(await backendIpc.getPluginMarketplace());
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    }, []);

    React.useEffect(() => void refresh(), [refresh]);

    const addSource = async (event: React.FormEvent) => {
        event.preventDefault();
        const url = sourceUrl.trim();
        if (!url) return;
        setBusy("add-source");
        try {
            setSnapshot(await backendIpc.addPluginMarketplaceSource(url));
            setSourceUrl("");
            pushToast(t("plugins.marketplace.source_added"), "success");
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    };

    const removeSource = async (url: string) => {
        setBusy(`remove:${url}`);
        try {
            setSnapshot(await backendIpc.removePluginMarketplaceSource(url));
            if (sourceFilter === url) setSourceFilter("");
            pushToast(t("plugins.marketplace.source_removed"), "success");
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    };

    const install = async (plugin: MarketplacePluginInfo) => {
        setBusy(`install:${plugin.sourceUrl}:${plugin.id}`);
        try {
            const next = await backendIpc.installMarketplacePlugin(plugin.sourceUrl, plugin.id);
            pushToast(t("plugins.installed"), "success");
            onInstalled(next, plugin.id);
        } catch (error) {
            pushToast(String(error), "error");
        } finally {
            setBusy(null);
        }
    };

    if (!snapshot) return <div className={styles.empty}>{t("plugins.marketplace.loading")}</div>;

    if (section === "sources") return (
        <div className={styles.sourcesPage}>
            <form className={styles.sourceForm} onSubmit={(event) => void addSource(event)}>
                <label htmlFor="plugin-marketplace-url">{t("plugins.marketplace.source_url")}</label>
                <div>
                    <input
                        id="plugin-marketplace-url"
                        type="url"
                        value={sourceUrl}
                        disabled={busy !== null}
                        placeholder="https://raw.githubusercontent.com/.../public/registry.json"
                        onChange={(event) => setSourceUrl(event.target.value)}
                    />
                    <button className="nav-btn" disabled={busy !== null || !sourceUrl.trim()} type="submit">
                        <span className="ms" aria-hidden="true">add</span>{t("plugins.marketplace.add_source")}
                    </button>
                    <button className="btn ghost" disabled={busy !== null} type="button" onClick={() => void refresh()}>
                        <span className="ms" aria-hidden="true">refresh</span>{t("plugins.marketplace.refresh")}
                    </button>
                </div>
            </form>
            <div className={styles.sources}>
                {snapshot.sources.map((source) => (
                    <article className={styles.sourceCard} key={source.url}>
                        <div>
                            <strong>{source.name || source.marketplaceId || t("plugins.marketplace.unknown_source")}</strong>
                            {source.default ? <span className={styles.badge}>{t("plugins.marketplace.default")}</span> : null}
                        </div>
                        <code>{source.url}</code>
                        {source.error ? <p className={styles.error}>{source.error}</p> : <p className={styles.success}>{t("plugins.marketplace.source_available")}</p>}
                        {!source.default ? <button className={styles.removeButton} disabled={busy !== null} onClick={() => void removeSource(source.url)}>
                            {t("plugins.marketplace.remove_source")}
                        </button> : null}
                    </article>
                ))}
            </div>
        </div>
    );

    const normalized = query.trim().toLocaleLowerCase();
    const installedIds = new Set(installed.map((plugin) => plugin.id));
    const categories = [...new Set(snapshot.plugins.flatMap((plugin) => plugin.categories))].sort();
    const plugins = snapshot.plugins.filter((plugin) => {
        const matchesQuery = !normalized || [plugin.name, plugin.id, plugin.description, plugin.author]
            .some((value) => value?.toLocaleLowerCase().includes(normalized));
        return matchesQuery
            && (!sourceFilter || plugin.sourceUrl === sourceFilter)
            && (!categoryFilter || plugin.categories.includes(categoryFilter));
    });

    return (
        <div>
            <div className={styles.filters}>
                <label className={styles.search}>
                    <span className="ms" aria-hidden="true">search</span>
                    <input value={query} placeholder={t("plugins.marketplace.search")} onChange={(event) => setQuery(event.target.value)}/>
                </label>
                <select value={sourceFilter} aria-label={t("plugins.marketplace.filter_source")} onChange={(event) => setSourceFilter(event.target.value)}>
                    <option value="">{t("plugins.marketplace.all_sources")}</option>
                    {snapshot.sources.filter((source) => !source.error).map((source) => <option key={source.url} value={source.url}>{source.name || source.marketplaceId}</option>)}
                </select>
                <select value={categoryFilter} aria-label={t("plugins.marketplace.filter_category")} onChange={(event) => setCategoryFilter(event.target.value)}>
                    <option value="">{t("plugins.marketplace.all_categories")}</option>
                    {categories.map((category) => <option key={category} value={category}>{category}</option>)}
                </select>
                <button className="btn ghost" disabled={busy !== null} onClick={() => void refresh()}>
                    <span className="ms" aria-hidden="true">refresh</span>{t("plugins.marketplace.refresh")}
                </button>
            </div>
            {snapshot.sources.some((source) => source.error) ? <div className={styles.sourceErrors} role="status">
                {snapshot.sources.filter((source) => source.error).map((source) => <p key={source.url}><strong>{source.name || source.url}</strong>: {source.error}</p>)}
            </div> : null}
            {plugins.length === 0 ? <div className={styles.empty}>
                <span className="ms" aria-hidden="true">extension_off</span>
                <strong>{t("plugins.marketplace.empty")}</strong>
            </div> : <div className={styles.grid}>
                {plugins.map((plugin) => {
                    const isInstalled = installedIds.has(plugin.id);
                    const blocked = plugin.installBlock;
                    const installKey = `install:${plugin.sourceUrl}:${plugin.id}`;
                    return <article className={styles.pluginCard} key={`${plugin.sourceUrl}:${plugin.id}`}>
                        <div className={styles.cardHeader}>
                            <span className={styles.icon}><span className="ms" aria-hidden="true">extension</span></span>
                            <div><h2>{plugin.name}</h2><code>{plugin.id}</code></div>
                            <span className={styles.badge} data-status={plugin.status}>{t(`plugins.marketplace.status.${plugin.status}`)}</span>
                        </div>
                        <p className={styles.description}>{plugin.description || t("plugins.marketplace.no_description")}</p>
                        <div className={styles.meta}>
                            <span>{t("plugins.version", {version: plugin.version})}</span>
                            <span>{t("plugins.api_version", {version: plugin.apiVersion})}</span>
                            <span>{plugin.marketplaceName}</span>
                            {plugin.author ? <span>{t("plugins.author", {author: plugin.author})}</span> : null}
                            {plugin.license ? <span>{plugin.license}</span> : null}
                            {plugin.minimumAppVersion ? <span>{t("plugins.marketplace.minimum_app_version", {version: plugin.minimumAppVersion})}</span> : null}
                            <span>{t("plugins.marketplace.published_at", {date: plugin.publishedAt})}</span>
                        </div>
                        {plugin.categories.length ? <div className={styles.categories}>{plugin.categories.map((category) => <span key={category}>{category}</span>)}</div> : null}
                        {plugin.status === "deprecated" ? <p className={styles.warning}>{t("plugins.marketplace.deprecated_hint")}</p> : null}
                        {blocked ? <p className={styles.warning}>{t(blockKey(blocked), {version: plugin.minimumAppVersion})}</p> : null}
                        {plugin.releaseNotes ? <details className={styles.notes}><summary>{t("plugins.marketplace.release_notes")}</summary><p>{plugin.releaseNotes}</p></details> : null}
                        <div className={styles.cardActions}>
                            <div className={styles.links}>
                                <button title={t("plugins.source")} aria-label={t("plugins.source")} onClick={() => void openUrl(plugin.source || plugin.repository)}><span className="ms" aria-hidden="true">code</span></button>
                                {plugin.homepage ? <button title={t("plugins.homepage")} aria-label={t("plugins.homepage")} onClick={() => void openUrl(plugin.homepage!)}><span className="ms" aria-hidden="true">home</span></button> : null}
                                {plugin.issues ? <button title={t("plugins.issues")} aria-label={t("plugins.issues")} onClick={() => void openUrl(plugin.issues!)}><span className="ms" aria-hidden="true">bug_report</span></button> : null}
                                {plugin.releaseNotesUrl ? <button title={t("plugins.marketplace.release_notes_link")} aria-label={t("plugins.marketplace.release_notes_link")} onClick={() => void openUrl(plugin.releaseNotesUrl!)}><span className="ms" aria-hidden="true">open_in_new</span></button> : null}
                            </div>
                            <button className="nav-btn" disabled={busy !== null || isInstalled || blocked !== null} onClick={() => void install(plugin)}>
                                {busy === installKey ? t("plugins.marketplace.installing") : isInstalled ? t("plugins.marketplace.installed") : t("plugins.marketplace.install")}
                            </button>
                        </div>
                    </article>;
                })}
            </div>}
        </div>
    );
}
