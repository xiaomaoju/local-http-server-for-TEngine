import { fetch as tauriFetch } from "@tauri-apps/plugin-http";
import WebSocket from "@tauri-apps/plugin-websocket";
import { RemoteApi } from "../../shared/api";
export * from "../../shared/api";
export const api = new RemoteApi(
  tauriFetch as typeof fetch,
  "desktop",
  async (url, message, closed) => {
    const socket = await WebSocket.connect(url);
    const unlisten = socket.addListener((event) => {
      if (event.type === "Text" && typeof event.data === "string")
        message(event.data);
      if (event.type === "Close") closed();
    });
    return {
      close: async () => {
        unlisten();
        await socket.disconnect();
      },
    };
  },
);
