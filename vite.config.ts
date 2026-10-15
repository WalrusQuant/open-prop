import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig, type Plugin } from "vite";

const host = process.env.TAURI_DEV_HOST;

// Cursor checks local dev ports for a Chrome DevTools endpoint. SvelteKit
// otherwise logs each miss as `[404] GET /json/version`.
function ignoreDebuggerProbe(): Plugin {
  return {
    name: "ignore-debugger-probe",
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const pathname = req.url?.split("?")[0];
        if (pathname === "/json" || pathname === "/json/list" || pathname === "/json/version") {
          res.statusCode = 404;
          res.end();
          return;
        }
        next();
      });
    },
  };
}

export default defineConfig({
  plugins: [ignoreDebuggerProbe(), sveltekit()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
});
