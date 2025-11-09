import React from "react";
import { useTranslation } from "react-i18next";
import { setAppLanguage } from "../lib/i18n";

const LANGS = [
    { value: "zh-CN", label: "简体中文" },
    { value: "ja-JP", label: "日本語" }
];

export default function LanguageSwitcher() {
    const { i18n } = useTranslation();
    const onChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
        setAppLanguage(e.target.value);
    };
    return (
        <select value={i18n.language} onChange={onChange} aria-label="language">
            {LANGS.map((l) => (
                <option key={l.value} value={l.value}>
                    {l.label}
                </option>
            ))}
        </select>
    );
}