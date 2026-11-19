const { defineConfig } = require("@playwright/test");

module.exports = defineConfig({ testDir: "playwright", testMatch: "broken.fixture.cjs", fullyParallel: true });
