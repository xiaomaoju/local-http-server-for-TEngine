import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

export default defineConfig({
  plugins: [vue()],
  resolve: { dedupe: ['vue', '@lucide/vue'] },
  server: {
    fs: { allow: ['..'] },
    port: 5174,
    proxy: {
      "/api": "http://localhost:8083",
      "/res": "http://localhost:8083",
      "/appversion": "http://localhost:8083",
      "/file": "http://localhost:8083",
    },
  },
});
