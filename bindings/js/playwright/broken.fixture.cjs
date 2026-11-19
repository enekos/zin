// An implementation with a defect, to check how a divergence is reported.
const { test } = require("@playwright/test");
const { verify } = require("zin-verify/playwright");
const { SPEC, adapter } = require("./editor.cjs");

verify(SPEC, adapter("stale"), { test, observeTimeout: 300 });
