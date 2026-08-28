import React from "react";
import ReactDOM from "react-dom/client";
import "./styles/theme.css";
import "./App.css";
import {applyTheme, readTheme, type ThemeMode} from "./lib/theme";
import {ensureI18nReady} from "./lib/i18n";
import PacketViewerWindow from "./windows/PacketViewerWindow";

const requestedTheme = new URLSearchParams(location.search).get("theme");
applyTheme(
    requestedTheme === "light"
        ? "auto"
        : (["dark", "dark-green", "dark-purple"].includes(requestedTheme ?? "")
            ? requestedTheme as ThemeMode
            : readTheme()),
);
window.addEventListener("storage", (event) => {
    if (event.key === "sl-theme") applyTheme(readTheme());
});

ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(<PacketViewerWindow/>);
});
