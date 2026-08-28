import i18n from "i18next";
import {initReactI18next} from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { locale } from "@tauri-apps/plugin-os";
import {setBackendLocale} from "./ipc";

import zhCN from "../locales/zh-CN.json";
import jaJP from "../locales/ja-JP.json";

async function tauriPreferredLocale(): Promise<string | null> {
    try {
        const l = await locale();
        return l || null;
    } catch {
        return null;
    }
}

const STORAGE_KEY = "sl-lang";

export async function ensureI18nReady() {
    let initialLng =
        (localStorage.getItem(STORAGE_KEY) as string | null) || undefined;

    if (!initialLng) {
        const sys = await tauriPreferredLocale();
        if (sys) initialLng = sys;
    }

    await i18n
        .use(LanguageDetector)
        .use(initReactI18next)
        .init({
            resources: {
                "zh-CN": {translation: zhCN},
                "ja-JP": {translation: jaJP}
            },
            fallbackLng: "zh-CN",
            lng: initialLng,
            interpolation: {escapeValue: false},
            detection: {
                order: ["localStorage", "querystring", "navigator", "htmlTag"],
                caches: ["localStorage"],
                lookupLocalStorage: STORAGE_KEY
            },
            returnNull: false
        });

    await setBackendLocale(i18n.resolvedLanguage || i18n.language);

    return i18n;
}

export function setAppLanguage(lng: string) {
    localStorage.setItem(STORAGE_KEY, lng);
    i18n.changeLanguage(lng);
    void setBackendLocale(lng);
}

export default i18n;
