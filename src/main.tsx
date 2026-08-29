import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { FloatingSurfaceProvider } from "./components/FloatingSurfaceProvider";
import "./styles/tokens.css";
import "./styles/tailwind.css";
import "streamdown/styles.css";
import "./styles/app.css";
import "./styles/setup-wizard.css";
import "./styles/apple.css";
import "./styles/composer-surfaces.css";
import "./styles/workbench.css";
import "./shared/ui/kit.css";
import {
  applyNativeWindowTheme,
  applyThemeToDocument,
  loadTheme,
} from "./lib/theme";

// Apply persisted theme before first paint of React tree.
const bootTheme = loadTheme(localStorage);
applyThemeToDocument(bootTheme);
// Sync macOS NSAppearance / vibrancy with app theme (avoids dark glass under light UI).
void applyNativeWindowTheme(bootTheme);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <FloatingSurfaceProvider>
      <App />
    </FloatingSurfaceProvider>
  </StrictMode>,
);
