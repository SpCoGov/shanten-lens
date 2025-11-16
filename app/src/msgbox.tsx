import React from "react";
import {createRoot} from "react-dom/client";
import "./styles/theme.css";
import "./fonts/material-symbols.css";
import {applyTheme, readTheme} from "./lib/theme";
import "./App.css";
import MsgBoxWindow from "./windows/MsgBoxWindow";
import {ensureI18nReady} from "./lib/i18n";

applyTheme(readTheme());
window.addEventListener("storage", (e) => {
    if (e.key === "sl-theme") applyTheme(readTheme());
});
ensureI18nReady().then(() => {
    const root = createRoot(document.getElementById("root")!);
    root.render(<MsgBoxWindow/>);
});
