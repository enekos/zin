// What a user writes: one adapter, and the specification does the rest.
import { describe, test } from "node:test";
import { verify } from "../index.js";
import { adapter } from "./bank.js";

verify(new URL("../../../examples/account.zin", import.meta.url), adapter(), { describe, test });
