const { readFileSync } = require("node:fs");
const { report } = require("./coverage.cjs");

// Playwright delivers the same attachments to a local reporter and merge-reports.
module.exports = class ZinReporter {
  constructor() { this.records = []; }
  onTestEnd(test, result) {
    for (const attachment of result.attachments) {
      if (attachment.name !== "zin-coverage") continue;
      const json = attachment.body ?? readFileSync(attachment.path);
      this.records.push({ source: test.parent.titlePath().slice(0, -1).join("/"), record: JSON.parse(json) });
    }
  }
  onEnd() { report(this.records); }
};
