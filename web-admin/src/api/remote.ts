import { RemoteApi } from "../../../shared/api";
export * from "../../../shared/api";
export const api = new RemoteApi(
  window.fetch.bind(window),
  "web",
  (url, message, closed) =>
    new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      let opened = false;
      const timeout = setTimeout(() => {
        socket.close();
        reject(new Error("实时连接超时"));
      }, 10000);
      socket.onmessage = (e) => message(e.data);
      socket.onopen = () => {
        opened = true;
        clearTimeout(timeout);
        resolve({
          close() {
            socket.onclose = null;
            socket.close();
          },
        });
      };
      socket.onerror = () => {
        if (!opened) {
          clearTimeout(timeout);
          reject(new Error("实时连接不可用"));
        }
      };
      socket.onclose = () => {
        clearTimeout(timeout);
        if (opened) closed();
        else reject(new Error("实时连接不可用"));
      };
    }),
);
