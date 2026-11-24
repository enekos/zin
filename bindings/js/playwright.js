// Share one implementation and error identity across ESM and CommonJS.
import binding from "./playwright.cjs";

export const { dialogs, permissions, visibility, messages, Channel, socketFrames, eventStream, clock, navigation, connectivity, actors, bind, button, response, role, oneOf, text, testId, Rejected, Gate, verify, actionable, click, fill, gate } = binding;
