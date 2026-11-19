import { defineConfig, type Plugin } from "vite";

const SAVE_DELAY_MS = 600;

/** Stands in for a backend under `npm run dev`; the tests answer the request themselves. */
const fakeApi: Plugin = {
  name: "fake-api",
  configureServer(server) {
    server.middlewares.use("/api/quantity", (_request, response) => {
      setTimeout(() => response.writeHead(204).end(), SAVE_DELAY_MS);
    });
  },
};

export default defineConfig({
  plugins: [fakeApi],
  test: { environment: "jsdom", include: ["tests/*.test.tsx"] },
});
