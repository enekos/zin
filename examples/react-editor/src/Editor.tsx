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

/** The implementation of ../../editor.zin: edit a quantity, save it, keep editing while it saves. */
export function Editor() {
  const [draft, setDraft] = useState(0);
  const [saved, setSaved] = useState(0);
  const [status, setStatus] = useState<Status>("idle");

  async function save() {
    const sent = draft;
    setStatus("saving");
    const ok = await putQuantity(sent).catch(() => false);
    if (ok) setSaved(sent);
    setStatus(ok ? "idle" : "failed");
  }

  return (
    <main>
      <button aria-label="Less" disabled={draft === 0} onClick={() => setDraft(draft - 1)}>
        −
      </button>
      <span data-testid="draft">{draft}</span>
      <button aria-label="More" disabled={draft === MAXIMUM} onClick={() => setDraft(draft + 1)}>
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
