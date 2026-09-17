import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { readCachedTheme } from "./lib/theme";
import "./styles.css";

// Before the first render, so the loading screen is already in the right theme.
document.documentElement.dataset.theme = readCachedTheme();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
