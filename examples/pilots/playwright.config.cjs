const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testMatch: ["browser.spec.mjs", "channels.spec.mjs"],
  workers: 1,
  timeout: 60_000,
});
