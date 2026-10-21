import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import PanelApp from "./PanelApp";
import { panelFromLabel } from "./components/panels";
import "./styles.css";

// 浮出去的面板窗口加载的还是这份 bundle，只按 label 认领自己该装哪块面板。
const panel = panelFromLabel(getCurrentWindow().label);

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>{panel ? <PanelApp panel={panel} /> : <App />}</StrictMode>,
);
