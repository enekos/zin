export type Scalar = number | boolean | string;
export type Observed = Record<string, Scalar | Record<string, Scalar> | Map<Scalar, Scalar>>;
export type Awaitable<T> = T | Promise<T>;

export interface Adapter {
  /** One function per action; arguments arrive as an object keyed by parameter name. Return `false` or throw a rejection to refuse. */
  actions: Record<string, (args: Record<string, Scalar>) => Awaitable<unknown>>;
  /** Which thrown errors mean "the implementation refused this action". */
  rejects?: (error: unknown) => boolean;
  /**
   * Any subset of the specification's state fields, each one complete: a map
   * lists every key and a record every member, at every depth. Await values
   * first; an observed promise, or an array in place of the object, fails the test.
   */
  observe?: () => Awaitable<Observed | null | undefined>;
  /** Awaited after every action, before the state is observed: wait here for the implementation to come to rest. */
  settle?: () => Awaitable<void>;
  teardown?: () => Awaitable<void>;
}

export type Factory = () => Awaitable<Adapter>;

export interface PlanTest {
  name: string;
  kind: "scenario" | "witness" | "tour" | "identify";
  /** `[call, to]`; `to` is -1 when the call must be rejected. */
  steps: [number, number][];
}

export interface Plan {
  zin: "plan/1";
  system: string;
  fingerprint: string;
  /** Included files by path relative to the specification, each with its FNV-1a 64 hash. */
  sources: Record<string, string>;
  /** The composition block tested in isolation, or null for the whole system. */
  component: string | null;
  cover: Cover;
  max_steps: number;
  step_budget: number;
  pair_limit: number | null;
  /** The planning options `zin plan` uses when none are given. */
  defaults: { cover: Cover; max_steps: number; step_budget: number; pair_limit: number | null };
  /** Present only in plans from `zin plan --observe`: the fields observe() is taken to return. */
  observe?: string[];
  fields: { name: string; type: unknown }[];
  actions: { name: string; params: { name: string; type: unknown }[] }[];
  states: Record<string, unknown>[];
  parents: ([number, number] | null)[];
  calls: { action: string; args: Scalar[]; text: string }[];
  tests: PlanTest[];
  summary: {
    states: number;
    transitions: number;
    refusals: number;
    eligible_pairs: number | null;
    covered_pairs: number | null;
    tests: number;
    accepted_steps: number;
    rejected_steps: number;
  };
}

export type Cover = "transitions" | "states" | "actions" | "scenarios" | "pairs";

/**
 * Planning options. Only the options passed are compared with a committed
 * plan's recorded ones; any that differ mean the plan must be derived again.
 */
export interface PlanOptions {
  cover?: Cover;
  /** A pre-derived plan file to use instead of running `zin plan`. */
  plan?: string | URL;
  /** Path of the `zin` binary; defaults to `$ZIN_BIN`, then `zin` on PATH. */
  bin?: string;
  /** Soft target for generated tour length (`zin plan --max-steps`). */
  maxSteps?: number;
  /** Hard limit on all emitted replay steps (`zin plan --step-budget`). */
  stepBudget?: number;
  /** With `cover: "pairs"`, how many adjacent-call pairs to sample (`zin plan --pair-limit`). */
  pairLimit?: number;
  /** Test one block of a composition in isolation (`zin plan --component`). */
  component?: string;
}

export interface RunOptions {
  /**
   * How long, in milliseconds, to keep observing until the state matches the
   * specification. 0 (the default) observes once: right for synchronous
   * implementations. Set it for one that settles on its own schedule, such as
   * a rendered user interface.
   */
  observeTimeout?: number;
  /** Pause between two observations, in milliseconds (default 20). */
  observeInterval?: number;
  /** Wraps every step, for frameworks that report steps; must return what `body` returns. */
  step?: <T>(text: string, body: () => Promise<T>) => Awaitable<T>;
}

// `any`, so that a framework's own `describe` and `test` types fit whatever they expect a body to return.
type Register = (name: string, body: () => any) => unknown;

export interface VerifyOptions extends PlanOptions, RunOptions {
  describe?: Register;
  test?: Register;
  it?: Register;
  /** The suite name. */
  name?: string;
}

export const PLAN_FORMAT: "plan/1";
export class Rejected extends Error {}
export class Divergence extends Error {
  /** The fingerprint of the specification the plan was derived from. */
  fingerprint: string;
  /** The original failing steps as a Zin scenario, ready to paste. */
  scenario: string;
}
export class PlanError extends Error {}
export function fingerprint(bytes: Uint8Array): string;
export function loadPlan(spec: string | URL, options?: PlanOptions): Plan;
export function runTest(plan: Plan, test: PlanTest, factory: Factory, options?: RunOptions): Promise<void>;
export function check(spec: string | URL, factory: Factory, options?: PlanOptions & RunOptions): Promise<Plan["summary"]>;
export function verify(spec: string | URL, factory: Factory, options?: VerifyOptions): Plan;

export class Recorder {
  /** A file to append to, a function that receives each line, or nothing to keep the lines in `lines`. */
  constructor(target?: string | URL | ((line: string) => void));
  readonly lines: string[];
  event(
    action: string,
    args?: Scalar[] | Record<string, Scalar>,
    options?: { accepted?: boolean; state?: Observed },
  ): void;
  reset(): void;
  text(): string;
}
