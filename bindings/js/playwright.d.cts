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
import type { Adapter, Awaitable, Plan, PlanOptions } from "./index.cjs";

export { Rejected } from "./index.cjs";

type BuiltIn = PlaywrightTestArgs & PlaywrightTestOptions & PlaywrightWorkerArgs & PlaywrightWorkerOptions;

export interface VerifyOptions<Fixtures> extends PlanOptions {
  /** The `test` to register with: yours when you extended it with fixtures. Defaults to the one from `@playwright/test`. */
  test?: TestType<any, any>;
  /** Fixtures the factory receives; default `["page"]`. */
  fixtures?: (keyof Fixtures & string)[];
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
