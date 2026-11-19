// The component against the specification, in jsdom: milliseconds per plan, no browser.
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { describe, test, vi } from "vitest";
import { Rejected, verify } from "zin-verify";
import { Editor } from "../src/Editor";

/** Clicks as a user would: a disabled button refuses the action. */
function press(name: string) {
  const button = screen.getByRole<HTMLButtonElement>("button", { name });
  if (button.disabled) throw new Rejected();
  fireEvent.click(button);
}

verify(
  // Not `new URL(..., import.meta.url)`: under jsdom, Vite resolves that to an http: address.
  join(import.meta.dirname, "../../editor.zin"),
  () => {
    // The requests the component has sent and nobody has answered yet.
    const waiting: ((response: Response) => void)[] = [];
    vi.stubGlobal("fetch", () => new Promise<Response>((resolve) => waiting.push(resolve)));
    render(<Editor />);

    async function answer(status: number) {
      const resolve = waiting.shift();
      if (!resolve) throw new Rejected();
      await act(async () => resolve(new Response(null, { status })));
    }

    return {
      actions: {
        increase: () => press("More"),
        decrease: () => press("Less"),
        save: () => press("Save"),
        server_ok: () => answer(204),
        server_fails: () => answer(500),
      },
      observe: () => ({
        draft: Number(screen.getByTestId("draft").textContent),
        saved: Number(screen.getByTestId("saved").textContent),
        status: screen.getByRole("status").textContent ?? "",
      }),
      teardown: () => {
        cleanup();
        vi.unstubAllGlobals();
      },
    };
  },
  { describe, test },
);
