import { useState } from "react";

const MAXIMUM = 2;

type Status = "idle" | "saving" | "failed";

async function putQuantity(quantity: number): Promise<boolean> {
  const response = await fetch("/api/quantity", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ quantity }),
  });
  return response.ok;
}

/**
 * The implementation of ../../editor.zin: edit a quantity, save it, keep editing while it saves.
 * `onEvent` hears every action of the specification as it takes effect.
 */
export function Editor({ onEvent }: { onEvent?: (action: string) => void }) {
  const [draft, setDraft] = useState(0);
  const [saved, setSaved] = useState(0);
  const [status, setStatus] = useState<Status>("idle");

  function change(by: number, action: string) {
    setDraft(draft + by);
    onEvent?.(action);
  }

  async function save() {
    const sent = draft;
    setStatus("saving");
    onEvent?.("save");
    const ok = await putQuantity(sent).catch(() => false);
    onEvent?.(ok ? "server_ok" : "server_fails");
    if (ok) setSaved(sent);
    setStatus(ok ? "idle" : "failed");
  }

  return (
    <main>
      <h1>Quantity editor</h1>
      <button aria-label="Less" disabled={draft === 0} onClick={() => change(-1, "decrease")}>
        −
      </button>
      <span data-testid="draft">{draft}</span>
      <button aria-label="More" disabled={draft === MAXIMUM} onClick={() => change(1, "increase")}>
        +
      </button>
      <button disabled={status === "saving" || draft === saved} onClick={save}>
        Save
      </button>
      <p>
        Saved: <span data-testid="saved">{saved}</span>
      </p>
      <p role="status">{status}</p>
    </main>
  );
}
