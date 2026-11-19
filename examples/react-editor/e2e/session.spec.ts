// No plan here: a session nobody scripted, recorded by the page itself and
// checked against the specification afterwards with `zin trace`.
import { execFileSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";

const SPEC = fileURLToPath(new URL("../../editor.zin", import.meta.url));
const ZIN = process.env.ZIN_BIN || "zin";
const CLICKS = 150;

/** A small seeded generator, so that a failing session can be run again. */
function random(seed: number) {
  return () => {
    seed = (seed * 1664525 + 1013904223) >>> 0;
    return seed / 2 ** 32;
  };
}

for (const seed of [1, 2, 3]) {
  test(`a random session is a behavior of the specification (seed ${seed})`, async ({ page }, testInfo) => {
    const next = random(seed);
    // A server that answers when it likes, and fails one time in four.
    await page.route("**/api/quantity", async (route) => {
      const status = next() < 0.25 ? 500 : 204;
      await new Promise((resolve) => setTimeout(resolve, next() * 40));
      await route.fulfill({ status });
    });
    await page.goto("/");

    for (let click = 0; click < CLICKS; click += 1) {
      // Chosen and clicked in one turn of the page, so that an answer
      // arriving in between cannot disable the chosen button.
      await page.evaluate((pick) => {
        const enabled = document.querySelectorAll<HTMLButtonElement>("button:enabled");
        enabled[Math.floor(pick * enabled.length)].click();
      }, next());
    }
    await expect(page.getByRole("status")).not.toHaveText("saving");

    const log = testInfo.outputPath("session.jsonl");
    writeFileSync(log, await page.evaluate(() => (window as any).zinSession.text()));
    await testInfo.attach("session.jsonl", { path: log });
    try {
      execFileSync(ZIN, ["trace", SPEC, log], { encoding: "utf8", stdio: "pipe" });
    } catch (error) {
      throw new Error((error as { stderr?: string }).stderr || String(error));
    }
  });
}
