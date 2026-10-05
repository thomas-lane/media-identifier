import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { BackendContext, getBackend } from "./api";
import { App } from "./App";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BackendContext.Provider value={getBackend()}>
      <App />
    </BackendContext.Provider>
  </StrictMode>,
);
