export type Scalar = number | boolean | string;
export type Observed = Record<string, Scalar | Record<string, Scalar> | Map<Scalar, Scalar>>;
export type Awaitable<T> = T | Promise<T>;

export interface Adapter {
  /** One function per action; arguments arrive as an object keyed by parameter name. Return `false` or throw a rejection to refuse. */
  actions: Record<string, (args: Record<string, Scalar>) => Awaitable<unknown>>;
  /** Which thrown errors mean "the implementation refused this action". */
  rejects?: (error: unknown) => boolean;
  /** Any subset of the specification's state fields. */
  observe?: () => Awaitable<Observed | null | undefined>;
  teardown?: () => Awaitable<void>;
}

export type Factory = () => Awaitable<Adapter>;

export interface PlanTest {
  name: string;
  kind: "scenario" | "tour";
  /** `[call, to]`; `to` is -1 when the call must be rejected. */
  steps: [number, number][];
}

export interface Plan {
  zin: "plan/1";
  system: string;
  fingerprint: string;
  cover: Cover;
  fields: { name: string; type: unknown }[];
  actions: { name: string; params: { name: string; type: unknown }[] }[];
  states: Record<string, unknown>[];
  parents: ([number, number] | null)[];
  calls: { action: string; args: Scalar[]; text: string }[];
  tests: PlanTest[];
  summary: { states: number; transitions: number; tests: number; accepted_steps: number; rejected_steps: number };
}

export type Cover = "transitions" | "states" | "actions" | "scenarios";

export interface PlanOptions {
  cover?: Cover;
  /** A pre-derived plan file to use instead of running `zin plan`. */
  plan?: string | URL;
  /** Path of the `zin` binary; defaults to `$ZIN_BIN`, then `zin` on PATH. */
  bin?: string;
}

type Register = (name: string, body: () => unknown) => unknown;

export interface VerifyOptions extends PlanOptions {
  describe?: Register;
  test?: Register;
  it?: Register;
  /** The suite name. */
  name?: string;
}

export const PLAN_FORMAT: "plan/1";
export class Rejected extends Error {}
export class Divergence extends Error {}
export class PlanError extends Error {}
export function fingerprint(bytes: Uint8Array): string;
export function loadPlan(spec: string | URL, options?: PlanOptions): Plan;
export function runTest(plan: Plan, test: PlanTest, factory: Factory): Promise<void>;
export function check(spec: string | URL, factory: Factory, options?: PlanOptions): Promise<Plan["summary"]>;
export function verify(spec: string | URL, factory: Factory, options?: VerifyOptions): Plan;

export class Recorder {
  constructor(target: string | URL | ((line: string) => void));
  event(
    action: string,
    args?: Scalar[] | Record<string, Scalar>,
    options?: { accepted?: boolean; state?: Observed },
  ): void;
  reset(): void;
}
