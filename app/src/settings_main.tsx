import "./styles/theme.css";
import "./App.css";
import "./fonts/material-symbols.css"; // ← 图标（见第3点）
import {applyTheme, readTheme} from "./lib/theme";
import SettingsWindow from "./windows/SettingsWindow";
import React from "react";
import ReactDOM from "react-dom/client";
import "./lib/i18n";
import {ensureI18nReady} from "./lib/i18n";

applyTheme(readTheme());

window.addEventListener("storage", (e) => {
    if (e.key === "sl-theme") applyTheme(readTheme());
});
ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(<SettingsWindow/>);
});