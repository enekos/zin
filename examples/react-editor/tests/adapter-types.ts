import { verify } from "zin-verify";
import type { Adapter } from "../e2e/editor-model";

declare const editor: Adapter;
verify("editor.zin", () => editor);

// An adapter with narrower action parameters can register with the generic runner.
verify("account.zin", () => ({ actions: { deposit: ({ amount }: { amount: number }) => amount } }));

// @ts-expect-error Unknown model action
editor.actions.publish({});
// @ts-expect-error The observation must respect the model's finite status values
const invalidObservation: Adapter["observe"] = () => ({ status: "published" });
