import {createRoot} from "react-dom/client";
import "./styles/theme.css";
import "./fonts/material-symbols.css";
import {initializeTheme} from "./lib/theme";
import "./App.css";
import MsgBoxWindow from "./windows/MsgBoxWindow";
import {ensureI18nReady} from "./lib/i18n";

initializeTheme();

function MsgBoxBootFallback() {
    return (
        <div
            style={{
                minHeight: "100vh",
                display: "grid",
                placeItems: "center",
                background: "var(--color-bg)",
                color: "var(--color-text)",
                fontSize: 13,
                letterSpacing: 0.2,
            }}
        >
            ...
        </div>
    );
}

const root = createRoot(document.getElementById("root")!);
root.render(<MsgBoxBootFallback/>);

ensureI18nReady()
    .catch(() => {
    })
    .finally(() => {
        root.render(<MsgBoxWindow/>);
    });
