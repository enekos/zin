// Compare actual frame commands. Playwright's HTML reporter hides fast reads
// such as isVisible and textContent, so count the pinned driver's wire log.
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";

const require = createRequire(import.meta.url);
async function measure(name) {
  const child = spawn(process.execPath, [require.resolve("@playwright/test/cli"),
    "test", "replay-cost.spec.mjs", "--grep", name, "--reporter=line"], {
    cwd: import.meta.dirname, env: { ...process.env, ZIN_MEASURE_COST: "1", DEBUG: "pw:channel", DEBUG_COLORS: "0" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let commands = 0;
  const tail = [];
  const count = async stream => {
    for await (const line of createInterface({ input: stream })) {
      if (/pw:channel SEND> .*?"guid":"frame@/.test(line)) commands++;
      if (!line.includes("pw:channel")) {
        tail.push(line);
        if (tail.length > 30) tail.shift();
      }
    }
  };
  const finished = new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", resolve);
  });
  const [status] = await Promise.all([finished, count(child.stdout), count(child.stderr)]);
  if (status !== 0 || !commands) throw new Error(`${name} failed to measure frame commands\n${tail.join("\n")}`);
  return commands;
}

const baseline = await measure("function adapter");
const bound = await measure("declarative adapter");
const reduction = 1 - bound / baseline;
console.log(`Editor replay: ${baseline} → ${bound} Playwright frame commands (${(100 * reduction).toFixed(1)}% fewer)`);
if (reduction < 0.4) process.exitCode = 1;
