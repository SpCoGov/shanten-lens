import React from "react";
import ReactDOM from "react-dom/client";
import {invoke} from "@tauri-apps/api/core";
import {createRoot} from "react-dom/client";
import App from "./App";

invoke("frontend_ready").catch(() => {
});
createRoot(document.getElementById("root")!).render(<App/>);

ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
        <App/>
    </React.StrictMode>
);