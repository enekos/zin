const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({
  testDir: "playwright",
  testMatch: "*.spec.cjs",
  fullyParallel: true,
  reporter: "line",
});
