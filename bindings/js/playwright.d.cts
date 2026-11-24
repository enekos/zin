import type {
  BrowserContext,
  Locator,
  Page,
  PlaywrightTestArgs,
  PlaywrightTestOptions,
  PlaywrightWorkerArgs,
  PlaywrightWorkerOptions,
  Request,
  Route,
  TestInfo,
  TestType,
} from "@playwright/test";
import type { Scalar, Observed, Adapter, Awaitable, Plan, PlanOptions } from "./index.cjs";

export { Rejected } from "./index.cjs";

type BuiltIn = PlaywrightTestArgs & PlaywrightTestOptions & PlaywrightWorkerArgs & PlaywrightWorkerOptions;

export interface VerifyOptions<Fixtures> extends PlanOptions {
  /** The `test` to register with: yours when you extended it with fixtures. Defaults to the one from `@playwright/test`. */
  test?: TestType<any, any>;
  /** Fixtures the factory receives; default `["page"]`. */
  fixtures?: (keyof Fixtures & string)[];
  /** Attach operation coverage for zin-verify/playwright-reporter or a merged blob report. */
  coverage?: boolean;
  /** Block ungated fetch/XHR/event streams and seed randomness on each page load. Use serviceWorkers: "block". */
  hermetic?: boolean | { seed?: number };
  /** The suite name. */
  name?: string;
  /** How long an observation may take to match the specification, in milliseconds (default 5000). */
  observeTimeout?: number;
  /** Pause between two observations, in milliseconds (default 20). */
  observeInterval?: number;
}

export function verify<Fixtures = BuiltIn>(
  spec: string | URL,
  factory: (fixtures: Fixtures, testInfo: TestInfo) => Awaitable<Adapter>,
  options?: VerifyOptions<Fixtures>,
): Plan;

export function actionable(locator: Locator): Promise<boolean>;
export function click(locator: Locator, options?: Parameters<Locator["click"]>[0]): Promise<void>;
export function fill(locator: Locator, value: string | number, options?: Parameters<Locator["fill"]>[1]): Promise<void>;

type Response = NonNullable<Parameters<Route["fulfill"]>[0]>;
type Match = (request: Request) => boolean;

export class Gate {
  readonly pending: number;
  readonly requests: Request[];
  request<T>(action: () => Awaitable<T>): Promise<T>;
  fulfill(response?: Response | ((request: Request) => Awaitable<Response>), match?: Match): Promise<void>;
  abort(errorCode?: string, match?: Match): Promise<void>;
  continue(overrides?: Parameters<Route["continue"]>[0], match?: Match): Promise<void>;
  dispose(): Promise<void>;
}

export function gate(
  target: Page | BrowserContext,
  url: string | RegExp | ((url: URL) => boolean),
  options?: { timeout?: number },
): Promise<Gate>;

type Scope = { page: Page; server?: Gate };
interface Button { kind: "button"; name: string; request?: boolean }
interface ResponseAction { kind: "response"; status: number }
export function button(name: string, options?: { request?: boolean }): Button;
export function oneOf<T extends string>(...values: T[]): (text: string) => T;
export function response(status: number): ResponseAction;
export function role(name: Parameters<Page["getByRole"]>[0]): (scope: Scope) => Promise<string>;
export function role<T>(name: Parameters<Page["getByRole"]>[0], parse: (text: string) => T): (scope: Scope) => Promise<T>;
export function testId(id: string): (scope: Scope) => Promise<string>;
export function testId<T>(id: string, parse: (text: string) => T): (scope: Scope) => Promise<T>;
export type Binding<A, S> = {
  url?: string;
  requests?: Parameters<typeof gate>[1];
  actions: { [K in keyof A]: Button | ResponseAction | ((scope: Scope, args: A[K] extends (args: infer P) => unknown ? P : never) => Awaitable<unknown>) };
  fields: { [K in keyof S]?: (scope: Scope) => Awaitable<S[K]> };
};
export function bind(config: {
  url?: string;
  requests?: Parameters<typeof gate>[1];
  actions: Record<string, Button | ResponseAction | { call(scope: Scope, args: Record<string, Scalar>): Awaitable<unknown> }["call"]>;
  fields: Record<string, (scope: Scope) => Awaitable<Observed[string]>>;
}): ((fixtures: { page: Page }) => Promise<Adapter>) & { fields: string[] };
