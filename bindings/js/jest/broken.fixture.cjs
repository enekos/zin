const { join } = require("node:path");
const { verify } = require("zin-verify");
const { adapter } = require("../test/bank.cjs");
verify(join(__dirname, "../../../examples/account.zin"), adapter("cap"));
