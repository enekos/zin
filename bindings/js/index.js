// Share one implementation and error identity across ESM and CommonJS.
import binding from "./index.cjs";

export const { PLAN_FORMAT, Rejected, Divergence, PlanError, fingerprint, loadPlan, runTest, check, verify, Recorder } = binding;
