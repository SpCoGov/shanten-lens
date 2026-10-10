import {useEffect, useState} from "react";
import {useTranslation} from "react-i18next";
import {
    CUSTOM_THEME_COLORS,
    createRandomTheme,
    readCustomTheme,
    readTheme,
    setCustomTheme,
    setTheme,
    subscribeThemeChanges,
    type ThemeMode,
} from "../lib/theme";
import styles from "../windows/SettingsWindow.module.css";

export default function ThemeSettings() {
    const {t} = useTranslation();
    const [mode, setMode] = useState(readTheme);
    const [custom, setCustom] = useState(readCustomTheme);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => subscribeThemeChanges(() => {
        setMode(readTheme());
        setCustom(readCustomTheme());
    }), []);

    const save = (update: () => void) => {
        try {
            update();
            setError(null);
        } catch (reason) {
            setError(String(reason));
        }
    };

    const computed = mode === "custom" ? getComputedStyle(document.documentElement) : null;

    return (
        <div className={`${styles.sectionBody} ${styles.themeSection}`}>
            <h3 className={styles.sectionTitle}>{t("settings.table.appearance")}</h3>
            <p className={styles.themeHint}>{t("settings.appearance.description")}</p>
            {error !== null ? <p className={styles.themeError} role="alert">{t("settings.save_failed", {reason: error})}</p> : null}
            <div className={styles.kvRows}>
                <div className={styles.kvRow}>
                    <label className={styles.settingCopy} htmlFor="theme.mode">
                        <span>{t("settings.appearance.theme")}</span>
                        <small>{t("settings.appearance.theme_desc")}</small>
                    </label>
                    <div className={styles.ctrl}>
                        <select id="theme.mode" className="form-input" value={mode}
                                onChange={(event) => save(() => setTheme(event.target.value as ThemeMode))}>
                            {["auto", "dark", "dark-green", ...(mode === "dark-purple" ? ["dark-purple"] : []), "custom"].map((theme) => (
                                <option key={theme} value={theme}>{t(`app.theme.${theme}`)}</option>
                            ))}
                        </select>
                    </div>
                </div>
                {mode === "custom" ? <>
                    <div className={styles.kvRow}>
                        <label className={styles.settingCopy} htmlFor="theme.base">
                            <span>{t("settings.appearance.base")}</span>
                            <small>{t("settings.appearance.base_desc")}</small>
                        </label>
                        <div className={styles.ctrl}>
                            <select id="theme.base" className="form-input" value={custom.base}
                                    onChange={(event) => save(() => setCustomTheme({...custom, base: event.target.value as "light" | "dark"}))}>
                                <option value="light">{t("settings.appearance.light")}</option>
                                <option value="dark">{t("app.theme.dark")}</option>
                            </select>
                        </div>
                    </div>
                    <div className={styles.themePaletteHeader}>
                        <h4>{t("settings.appearance.colors_title")}</h4>
                        <div className={styles.themePaletteActions}>
                            <button type="button" className="btn ghost"
                                    onClick={() => save(() => setCustomTheme(createRandomTheme(custom.base)))}>
                                {t("settings.appearance.randomize")}
                            </button>
                            <button type="button" className="btn ghost" disabled={!Object.keys(custom.colors).length}
                                    onClick={() => save(() => setCustomTheme({...custom, colors: {}}))}>
                                {t("settings.appearance.reset")}
                            </button>
                        </div>
                    </div>
                    <div className={styles.themeColors}>
                        {CUSTOM_THEME_COLORS.map(({key, label}) => {
                            const value = custom.colors[key] ?? computed!.getPropertyValue(key).trim();
                            const color = value.length === 4 ? `#${[...value.slice(1)].map((digit) => digit + digit).join("")}` : value;
                            const id = `theme.color.${label}`;
                            return (
                                <label className={styles.themeColor} htmlFor={id} key={key}>
                                    <span className={styles.settingCopy}>
                                        <span>{t(`settings.appearance.colors.${label}`)}</span>
                                        <small>{color.toUpperCase()}</small>
                                    </span>
                                    <input id={id} type="color" value={color}
                                           onChange={(event) => save(() => setCustomTheme({...custom, colors: {...custom.colors, [key]: event.target.value}}))}/>
                                </label>
                            );
                        })}
                    </div>
                </> : null}
            </div>
        </div>
    );
}
