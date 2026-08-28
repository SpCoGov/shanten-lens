import React from "react";
import {useTranslation} from "react-i18next";
import {setAppLanguage} from "../lib/i18n";
import {safeEmit} from "../lib/tauriRuntime";

const LANGS = ["zh-CN", "ja-JP"] as const;

export default function LanguageSwitcher() {
    const {i18n, t} = useTranslation();

    const onChange = async (e: React.ChangeEvent<HTMLSelectElement>) => {
        const lng = e.target.value;
        setAppLanguage(lng);
        await safeEmit("i18n:set-language", {lng});
    };
    return (
        <select value={i18n.resolvedLanguage || i18n.language} onChange={onChange} aria-label={t("language.selector")}>
            {LANGS.map((language) => (
                <option key={language} value={language}>{t(`language.${language}`)}</option>
            ))}
        </select>
    );
}
