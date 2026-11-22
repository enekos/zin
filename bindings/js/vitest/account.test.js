import { verify } from "zin-verify/vitest";
import { adapter } from "../test/bank.js";

verify(new URL("../../../examples/account.zin", import.meta.url), adapter());
