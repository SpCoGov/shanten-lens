import ReactDOM from "react-dom/client";
import "./styles/theme.css";
import "./App.css";
import {initializeTheme} from "./lib/theme";
import {ensureI18nReady} from "./lib/i18n";
import PacketViewerWindow from "./windows/PacketViewerWindow";

initializeTheme();

ensureI18nReady().then(() => {
    ReactDOM.createRoot(document.getElementById("root")!).render(<PacketViewerWindow/>);
});
