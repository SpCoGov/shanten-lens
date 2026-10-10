import "./styles/theme.css";
import "./App.css";
import "./fonts/material-symbols.css";
import {initializeTheme} from "./lib/theme";
import SettingsWindow from "./windows/SettingsWindow";
import ReactDOM from "react-dom/client";
import "./lib/i18n";
import {ensureI18nReady} from "./lib/i18n";

initializeTheme();
ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(<SettingsWindow/>);
});
