import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, type TestInfo } from "@playwright/test";
import type { Observed } from "zin-verify";

export async function inspectState(_state: Observed, page: Page, { id, testInfo }: { id: number; testInfo: TestInfo }) {
  await testInfo.attach(`state-${id}.png`, { body: await page.screenshot(), contentType: "image/png" });
  const audit = await new AxeBuilder({ page }).analyze();
  expect(audit.violations.map(({ id, nodes }) => ({ id, targets: nodes.map(node => node.target) }))).toEqual([]);
}
