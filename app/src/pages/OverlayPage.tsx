import React from "react";
import {invoke} from "@tauri-apps/api/core";
import {t} from "i18next";
import {
    readHudEnabled,
    readHudShowBlackhole,
    readHudShowScoreProjection,
    readHudShowWanxiang,
    writeHudEnabled,
    writeHudShowBlackhole,
    writeHudShowScoreProjection,
    writeHudShowWanxiang,
} from "../lib/hudSettings";

type OverlayStatus = {
    enabled: boolean;
    supported: boolean;
    found: boolean;
    pid: number | null;
};

export default function OverlayPage() {
    const [status, setStatus] = React.useState<OverlayStatus | null>(null);
    const [saving, setSaving] = React.useState(false);
    const [showBlackhole, setShowBlackhole] = React.useState(() => readHudShowBlackhole());
    const [showWanxiang, setShowWanxiang] = React.useState(() => readHudShowWanxiang());
    const [showScoreProjection, setShowScoreProjection] = React.useState(() => readHudShowScoreProjection());

    const refresh = React.useCallback(() => {
        invoke<OverlayStatus>("get_overlay_status")
            .then(setStatus)
            .catch(() => setStatus({enabled: false, supported: false, found: false, pid: null}));
    }, []);

    React.useEffect(() => {
        const savedEnabled = readHudEnabled();
        invoke<OverlayStatus>("set_overlay_enabled", {enabled: savedEnabled})
            .then(setStatus)
            .catch(() => setStatus({enabled: false, supported: false, found: false, pid: null}));
        refresh();
        const timer = window.setInterval(refresh, 800);
        return () => window.clearInterval(timer);
    }, [refresh]);

    const toggleEnabled = async (enabled: boolean) => {
        writeHudEnabled(enabled);
        setSaving(true);
        try {
            const next = await invoke<OverlayStatus>("set_overlay_enabled", {enabled});
            setStatus(next);
        } catch {
            setStatus({enabled: false, supported: false, found: false, pid: null});
        } finally {
            setSaving(false);
        }
    };

    const toggleShowBlackhole = (enabled: boolean) => {
        setShowBlackhole(enabled);
        writeHudShowBlackhole(enabled);
    };

    const toggleShowScoreProjection = (enabled: boolean) => {
        setShowScoreProjection(enabled);
        writeHudShowScoreProjection(enabled);
    };

    const toggleShowWanxiang = (enabled: boolean) => {
        setShowWanxiang(enabled);
        writeHudShowWanxiang(enabled);
    };

    const processText = status === null
        ? t("overlay.status_checking")
        : status.found
            ? t("overlay.process_found", {pid: status.pid})
            : t("overlay.process_not_found");

    return (
        <div className="settings-wrap wide-page config-page overlay-page">
            <header className="config-page-header">
                <div>
                    <h1>{t("overlay.title")}</h1>
                    <p>{t("overlay.subtitle")}</p>
                </div>
            </header>

            {status?.supported === false ? <div className="notice">{t("overlay.windows_only")}</div> : null}

            <section className={`overlay-status-card ${status?.enabled ? "is-enabled" : ""}`}>
                <div className="overlay-status-copy">
                    <h2>{t("overlay.enabled")}</h2>
                    <div className="overlay-process-status">
                        <span>{t("overlay.process_status")}</span>
                        <span className={`badge ${status?.found ? "ok" : ""}`}>{processText}</span>
                    </div>
                </div>
                <input
                    type="checkbox"
                    className="config-switch is-large"
                    aria-label={t("overlay.enabled")}
                    checked={Boolean(status?.enabled)}
                    disabled={saving || status?.supported === false}
                    onChange={(event) => void toggleEnabled(event.currentTarget.checked)}
                />
            </section>

            <div className="overlay-section-heading">
                <h2>{t("overlay.display_title")}</h2>
                <p>{t("overlay.display_desc")}</p>
            </div>
            <div className="overlay-option-grid">
                <label className="overlay-option-card">
                    <span>{t("overlay.show_blackhole")}</span>
                    <input type="checkbox" className="config-switch" checked={showBlackhole} onChange={(event) => toggleShowBlackhole(event.currentTarget.checked)}/>
                </label>
                <label className="overlay-option-card">
                    <span>{t("overlay.show_wanxiang")}</span>
                    <input type="checkbox" className="config-switch" checked={showWanxiang} onChange={(event) => toggleShowWanxiang(event.currentTarget.checked)}/>
                </label>
                <label className="overlay-option-card">
                    <span>{t("overlay.show_score_projection")}</span>
                    <input type="checkbox" className="config-switch" checked={showScoreProjection} onChange={(event) => toggleShowScoreProjection(event.currentTarget.checked)}/>
                </label>
            </div>
        </div>
    );
}
