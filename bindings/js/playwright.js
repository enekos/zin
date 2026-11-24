// Share one implementation and error identity across ESM and CommonJS.
import binding from "./playwright.cjs";

export const { bind, button, response, role, oneOf, testId, Rejected, Gate, verify, actionable, click, fill, gate } = binding;
