import type { Awaitable } from "zin-verify";

export type State = {
  "draft": number;
  "saved": number;
  "sent": number;
  "status": "idle" | "saving" | "failed";
};

export type Actions = {
  "increase": (args: {  }) => Awaitable<unknown>;
  "decrease": (args: {  }) => Awaitable<unknown>;
  "save": (args: {  }) => Awaitable<unknown>;
  "server_ok": (args: {  }) => Awaitable<unknown>;
  "server_fails": (args: {  }) => Awaitable<unknown>;
};

export type Adapter = { actions: Actions; observe: () => Awaitable<Partial<State>> };
