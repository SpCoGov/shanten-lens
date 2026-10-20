import i18n from "./i18n";
import * as backendIpc from "./ipc";
import {pushToast, type ToastKind} from "./toast";
import {registerPluginPage, usePluginStore, type PluginPage} from "./pluginStore";
import {safelyCleanup} from "./pluginLifecycle";

export type FrontendPluginPage = Omit<PluginPage, "id" | "pluginId"> & {id: string};

export type FrontendPluginApi = {
    pluginId: string;
    theme: {current(): string; onChange(callback: () => void): () => void};
    i18n: {locale(): string; t(key: string, values?: Record<string, unknown>): string; onChange(callback: () => void): () => void};
    config: {get(): Promise<Record<string, unknown>>; set(value: Record<string, unknown>): Promise<void>};
    backend: {
        invoke<T>(method: string, params?: unknown): Promise<T>;
        request<T>(moduleId: string, method: string, payload: unknown): Promise<T>;
        onEvent(name: string, callback: (data: unknown) => void): () => void;
    };
    ui: {registerPage(page: FrontendPluginPage): () => void; openPage(id: string): void; showToast(message: string, kind?: ToastKind): void};
};

export function createPluginApi(pluginId: string, namespace: string, defaultLocale: string) {
    const cleanups = new Set<() => void>();
    let disposed = false;
    const ensureActive = () => {
        if (disposed) throw new Error(i18n.t("plugins.context_disposed"));
    };
    const track = (cleanup: () => void) => {
        let active = true;
        const once = () => {
            if (!active) return;
            active = false;
            cleanups.delete(once);
            cleanup();
        };
        cleanups.add(once);
        return once;
    };
    const api: FrontendPluginApi = {
        pluginId,
        theme: {
            current: () => document.documentElement.dataset.theme || "auto",
            onChange: (callback) => {
                ensureActive();
                const observer = new MutationObserver(callback);
                observer.observe(document.documentElement, {attributes: true, attributeFilter: ["data-theme", "data-custom-theme"]});
                return track(() => observer.disconnect());
            },
        },
        i18n: {
            locale: () => i18n.resolvedLanguage || i18n.language,
            t: (key, values) => {
                const locale = i18n.resolvedLanguage || i18n.language;
                const fallback = String(i18n.getFixedT(defaultLocale, namespace)(key, values));
                return String(i18n.getFixedT(locale, namespace)(key, {...values, defaultValue: fallback}));
            },
            onChange: (callback) => {
                ensureActive();
                i18n.on("languageChanged", callback);
                return track(() => i18n.off("languageChanged", callback));
            },
        },
        config: {
            get: async () => { ensureActive(); return (await backendIpc.getPluginConfig(pluginId)) as Record<string, unknown>; },
            set: async (value) => { ensureActive(); await backendIpc.setPluginConfig(pluginId, value as backendIpc.JsonValue); },
        },
        backend: {
            invoke: async <T,>(method: string, params: unknown = null) => { ensureActive(); return await backendIpc.invokePlugin(pluginId, method, params as backendIpc.JsonValue) as T; },
            request: async <T,>(moduleId: string, method: string, payload: unknown) => {
                ensureActive();
                return await backendIpc.requestPluginPacket(pluginId, moduleId, method, payload as backendIpc.JsonValue) as T;
            },
            onEvent: (name, callback) => {
                ensureActive();
                return track(backendIpc.subscribeBackendEvent("plugin_event", event => {
                    if (event.pluginId === pluginId && event.name === name) callback(event.data);
                }));
            },
        },
        ui: {
            openPage: id => {
                ensureActive();
                const pageId = `${pluginId}:${id}`;
                if (usePluginStore.getState().pages.some(page => page.id === pageId)) {
                    window.dispatchEvent(new CustomEvent("sl:plugin-page", {detail: pageId}));
                }
            },
            registerPage: (page) => {
                ensureActive();
                if (!page.id || !page.title || typeof page.mount !== "function") throw new Error("invalid plugin page");
                return track(registerPluginPage({
                    ...page,
                    id: `${pluginId}:${page.id}`,
                    pluginId,
                    icon: page.icon || "extension",
                }));
            },
            showToast: (message, kind = "info") => { ensureActive(); pushToast(String(message), kind); },
        },
    };
    return {api, cleanup: () => {
        disposed = true;
        [...cleanups].reverse().forEach(safelyCleanup);
    }};
}
