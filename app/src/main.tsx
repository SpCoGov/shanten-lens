import React from "react";
import ReactDOM from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import App from "./App";
import { ensureI18nReady } from "./lib/i18n";

invoke("frontend_ready").catch(() => {
});
ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>
    );
});
