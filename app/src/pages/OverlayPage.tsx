import React from "react";
import {invoke} from "@tauri-apps/api/core";
import {t} from "i18next";
import {
    readHudShowBlackhole,
    readHudShowScoreProjection,
    writeHudShowBlackhole,
    writeHudShowScoreProjection,
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
    const [showScoreProjection, setShowScoreProjection] = React.useState(() => readHudShowScoreProjection());

    const refresh = React.useCallback(() => {
        invoke<OverlayStatus>("get_overlay_status")
            .then(setStatus)
            .catch(() => setStatus({enabled: false, supported: false, found: false, pid: null}));
    }, []);

    React.useEffect(() => {
        refresh();
        const timer = window.setInterval(refresh, 800);
        return () => window.clearInterval(timer);
    }, [refresh]);

    const toggleEnabled = async (enabled: boolean) => {
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

    return (
        <div className="settings-wrap wide-page">
            <div className="settings-header">
                <h2 className="title">{t("overlay.title")}</h2>
            </div>

            {status?.supported === false ? (
                <div className="notice">{t("overlay.windows_only")}</div>
            ) : null}

            <div className="panel">
                <div className="panel-title">{t("overlay.title")}</div>
                <div className="rows">
                    <div className="row">
                        <label>{t("overlay.enabled")}</label>
                        <input
                            type="checkbox"
                            className="form-checkbox"
                            checked={Boolean(status?.enabled)}
                            disabled={saving || status?.supported === false}
                            onChange={(event) => void toggleEnabled(event.currentTarget.checked)}
                        />
                    </div>
                    <div className="row">
                        <label>{t("overlay.process_status")}</label>
                        <span className={`badge ${status?.found ? "ok" : ""}`}>
                            {status === null
                                ? t("overlay.status_checking")
                                : status.found
                                    ? t("overlay.process_found", {pid: status.pid})
                            : t("overlay.process_not_found")}
                        </span>
                    </div>
                    <div className="row">
                        <label>{t("overlay.show_blackhole")}</label>
                        <input
                            type="checkbox"
                            className="form-checkbox"
                            checked={showBlackhole}
                            onChange={(event) => toggleShowBlackhole(event.currentTarget.checked)}
                        />
                    </div>
                    <div className="row">
                        <label>{t("overlay.show_score_projection")}</label>
                        <input
                            type="checkbox"
                            className="form-checkbox"
                            checked={showScoreProjection}
                            onChange={(event) => toggleShowScoreProjection(event.currentTarget.checked)}
                        />
                    </div>
                </div>
            </div>
        </div>
    );
}
