import i18n from "./i18n";
import {convertFileSrc} from "@tauri-apps/api/core";
import * as backendIpc from "./ipc";
import {createPluginApi, type FrontendPluginApi} from "./pluginApi";
import {clearPluginUi, setPluginFrontendError} from "./pluginStore";
import {withPluginDeadline, safelyCleanup} from "./pluginLifecycle";

type ActiveFrontend = {
    signature: string;
    controller: AbortController;
    cleanup(): void;
};

const active = new Map<string, ActiveFrontend>();
let generation = 0;

function deactivate(pluginId: string) {
    const current = active.get(pluginId);
    if (!current) return;
    active.delete(pluginId);
    current.controller.abort();
    safelyCleanup(current.cleanup);
    clearPluginUi(pluginId);
    setPluginFrontendError(pluginId);
}

function activate(bundle: backendIpc.PluginFrontendBundle) {
    const signature = JSON.stringify([bundle.entryPath, bundle.source, bundle.locales, bundle.defaultLocale, bundle.generation]);
    if (active.get(bundle.pluginId)?.signature === signature) return;
    deactivate(bundle.pluginId);
    const namespace = `plugin-${bundle.pluginId}`;
    const locales = Object.keys(bundle.locales);
    const context = createPluginApi(bundle.pluginId, namespace, bundle.defaultLocale);
    const controller = new AbortController();
    let returnedCleanup: (() => void) | undefined;
    const slot: ActiveFrontend = {
        signature, controller,
        cleanup: () => {
            safelyCleanup(returnedCleanup);
            returnedCleanup = undefined;
            context.cleanup();
            locales.forEach((locale) => i18n.removeResourceBundle(locale, namespace));
        },
    };
    active.set(bundle.pluginId, slot);
    const report = (error: string | null) => {
        if (bundle.healthToken) void backendIpc.reportPluginFrontendHealth(bundle.pluginId, bundle.healthToken, error)
            .catch((reason) => console.error("plugin health report failed", reason));
    };
    const load = async () => {
        if (bundle.error || !bundle.source || !bundle.entryPath) throw new Error(bundle.error || i18n.t("plugins.source_empty"));
        for (const [locale, resources] of Object.entries(bundle.locales)) {
            i18n.addResourceBundle(locale, namespace, resources, true, true);
        }
        // A fresh URL also allows retrying a previously rejected ES module import.
        const url = `${convertFileSrc(bundle.entryPath)}?v=${crypto.randomUUID()}`;
        const module = await import(/* @vite-ignore */ url) as {activate?: (api: FrontendPluginApi) => unknown};
        if (controller.signal.aborted) return;
        if (typeof module.activate !== "function") throw new Error(i18n.t("plugins.activate_missing"));
        const result = await module.activate(context.api);
        if (typeof result === "function") {
            if (controller.signal.aborted) safelyCleanup(result as () => void);
            else returnedCleanup = result as () => void;
        }
    };
    void withPluginDeadline(load(), controller.signal, i18n.t("plugins.activation_timeout")).then(() => {
        if (active.get(bundle.pluginId) !== slot || controller.signal.aborted) return;
        setPluginFrontendError(bundle.pluginId);
        report(null);
    }).catch((error) => {
        if (active.get(bundle.pluginId) !== slot || controller.signal.aborted) return;
        controller.abort();
        safelyCleanup(slot.cleanup);
        clearPluginUi(bundle.pluginId);
        const message = error instanceof Error ? error.message : String(error);
        setPluginFrontendError(bundle.pluginId, message);
        report(message);
    });
}

export function startPluginRuntime() {
    let stopped = false;
    const sync = () => {
        const revision = ++generation;
        void backendIpc.getPluginFrontends().then((bundles) => {
            if (stopped || revision !== generation) return;
            const expected = new Set(bundles.map((bundle) => bundle.pluginId));
            [...active.keys()].filter((id) => !expected.has(id)).forEach(deactivate);
            bundles.forEach(activate);
        }).catch((error) => console.error("plugin frontend sync failed", error));
    };
    const stop = backendIpc.subscribeBackendEvent("plugin_status", (plugins) => {
        const enabled = new Set(plugins.filter((plugin) => plugin.enabled && plugin.hasFrontend).map((plugin) => plugin.id));
        [...active.keys()].filter((id) => !enabled.has(id)).forEach(deactivate);
        sync();
    });
    sync();
    return () => {
        stopped = true;
        ++generation;
        stop();
        [...active.keys()].forEach(deactivate);
    };
}
