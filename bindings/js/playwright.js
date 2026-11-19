// Share one implementation and error identity across ESM and CommonJS.
import binding from "./playwright.cjs";

export const { Rejected, Gate, verify, actionable, click, fill, gate } = binding;
