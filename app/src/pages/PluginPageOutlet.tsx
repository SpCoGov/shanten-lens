import React from "react";
import {useTranslation} from "react-i18next";
import type {PluginPage} from "../lib/pluginStore";
import styles from "./PluginPageOutlet.module.css";
import {safelyCleanup, withPluginDeadline} from "../lib/pluginLifecycle";
import {setPluginFrontendError} from "../lib/pluginStore";

export default function PluginPageOutlet({page}: {page?: PluginPage}) {
    const {t} = useTranslation();
    const container = React.useRef<HTMLDivElement>(null);
    const [error, setError] = React.useState<string>();

    React.useEffect(() => {
        const outlet = container.current;
        if (!outlet || !page) return;
        const element = document.createElement("div");
        element.className = styles.container;
        outlet.replaceChildren(element);
        let disposed = false;
        const controller = new AbortController();
        let cleanup: void | (() => void);
        setError(undefined);
        const mounting = Promise.resolve().then(() => disposed ? undefined : page.mount(element)).then((result) => {
            if (disposed && typeof result === "function") safelyCleanup(result);
            else cleanup = result;
        });
        void withPluginDeadline(mounting, controller.signal, t("plugins.mount_timeout")).catch((reason) => {
            if (!disposed) {
                disposed = true;
                element.remove();
                const message = reason instanceof Error ? reason.message : String(reason);
                setError(message);
                setPluginFrontendError(page.pluginId, message);
            }
        });
        return () => {
            disposed = true;
            controller.abort();
            safelyCleanup(cleanup || undefined);
            element.remove();
        };
    }, [page]);

    if (!page) return <div className={styles.error}>{t("plugins.page_missing")}</div>;
    return (
        <div className={styles.page}>
            {error ? <div className={styles.error}><strong>{t("plugins.frontend_error")}</strong><pre>{error}</pre></div> : null}
            <div className={styles.container} ref={container}/>
        </div>
    );
}
