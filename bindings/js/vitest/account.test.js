// Vitest without globals: pass its describe/test, as with node:test.
import { describe, test } from "vitest";
import { verify } from "zin-verify";
import { adapter } from "../test/bank.js";

verify(new URL("../../../examples/account.zin", import.meta.url), adapter(), { describe, test });
