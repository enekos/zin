// What a user writes: one adapter, and the specification does the rest.
const { test } = require("@playwright/test");
const { verify } = require("zin-verify/playwright");
const { SPEC, adapter } = require("./editor.cjs");

verify(SPEC, adapter(), { test });
