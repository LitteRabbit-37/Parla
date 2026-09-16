import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { MiniRecorderView } from "./components/MiniRecorderView";
import { RecorderPopoverView } from "./components/RecorderPopoverView";
import "./i18n";
import "./index.css";

// Selection de la vue en fonction du label de fenetre :
// - "main" : app principale
// - "recorder" : mini-recorder flottant
// - "recorder-popover" : popover du bouton Mode de la bulle
const label = getCurrentWindow().label;
const isRecorder = label === "recorder";
const isPopover = label === "recorder-popover";
const Root = isRecorder ? MiniRecorderView : isPopover ? RecorderPopoverView : App;

// Les fenetres bulle et popover sont transparentes au niveau Tauri
// (decorations off, transparent: true). On force le body / #root en
// transparent pour eviter que le bg blanc tailwind par defaut colle un
// carre blanc derriere la pill.
if (isRecorder || isPopover) {
  document.documentElement.style.background = "transparent";
  document.body.style.background = "transparent";
}
if (isRecorder) {
  document.body.style.colorScheme = "dark";
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
