import type { Factory, Plan, PlanOptions, RunOptions } from "./index.cjs";

export function verify(spec: string | URL, factory: Factory, options?: PlanOptions & RunOptions & { name?: string }): Plan;
