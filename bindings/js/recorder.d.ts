import type { Observed, Scalar } from "./index.js";

/** Writes the events of a running system in the format `zin trace` reads. Has no Node.js dependency. */
export class Recorder {
  /** `write` receives each line; without it the lines are kept in `lines`. */
  constructor(write?: (line: string) => void);
  /** The lines recorded so far, when no `write` was given. */
  readonly lines: string[];
  event(
    action: string,
    args?: Scalar[] | Record<string, Scalar>,
    options?: { accepted?: boolean; state?: Observed },
  ): void;
  /** Marks the start of a new run from the initial state. */
  reset(): void;
  /** The kept lines as one log. */
  text(): string;
}
