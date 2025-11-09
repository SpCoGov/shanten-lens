import React from "react";
import ReactDOM from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import {createRoot} from "react-dom/client";
import App from "./App";
import { ensureI18nReady } from "./lib/i18n";

invoke("frontend_ready").catch(() => {
});
createRoot(document.getElementById("root")!).render(<App/>);

ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(
        <React.StrictMode>
            <App />
        </React.StrictMode>
    );
});
