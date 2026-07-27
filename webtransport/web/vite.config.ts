import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// 開発サーバ。ページは http://localhost:5173 で配信する。
// localhost は http でも「安全なコンテキスト」扱いなので、WebTransport の
// serverCertificateHashes(自己署名ピン留め)がそのまま使える。
// public/cert-hash.hex(relay が書き出す)は `/cert-hash.hex` で配信される。
export default defineConfig({
  plugins: [react()],
  server: {
    host: "localhost",
    port: 5173,
  },
});
