import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1520,
    strictPort: true,
  },
  preview: {
    host: "127.0.0.1",
    port: 1521,
    strictPort: true,
  },
});
