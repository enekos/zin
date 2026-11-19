import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Recorder } from "zin-verify/recorder";
import { Editor } from "./Editor";

// What the page did, as a log `zin trace` checks: see e2e/session.spec.ts.
// A deployed application would send the lines to its backend instead.
const session = new Recorder();
Object.assign(window, { zinSession: session });

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Editor onEvent={(action) => session.event(action)} />
  </StrictMode>,
);
