import { describe, test } from "@jest/globals";
import { verify } from "zin-verify";
import { adapter } from "../test/bank.js";

verify(new URL("../../../examples/account.zin", import.meta.url), adapter(), { describe, test });
