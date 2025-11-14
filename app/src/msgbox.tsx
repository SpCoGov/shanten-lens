import React from "react";
import { createRoot } from "react-dom/client";
import "./styles/theme.css";
import "./App.css"; // 复用按钮等通用样式
import MsgBoxWindow from "./windows/MsgBoxWindow";

const root = createRoot(document.getElementById("root")!);
root.render(<MsgBoxWindow />);
