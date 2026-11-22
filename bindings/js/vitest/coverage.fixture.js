import { verify } from "zin-verify/vitest";
verify(new URL("../../../tests/fixtures/v0.2/switch.plan.json", import.meta.url), () => {
  let active = false;
  return {
    actions: {
      start() { active = true; }, // deliberately accepts the second start
      stop() { active = false; },
    },
    observe: () => ({ active }),
  };
});
