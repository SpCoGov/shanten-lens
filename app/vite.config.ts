import {defineConfig} from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from "path";
import {readFileSync} from "fs";

const LIQI_METHODS_ID = "\0virtual:liqi-methods";
const APP_VERSION = readFileSync(resolve(__dirname, "../Cargo.toml"), "utf8")
    .match(/^version\s*=\s*"([^"]+)"/m)?.[1];

if (!APP_VERSION) throw new Error("Workspace version is missing from Cargo.toml");

const liqiMethods = () => ({
    name: "liqi-methods",
    resolveId(id: string) {
        return id === "virtual:liqi-methods" ? LIQI_METHODS_ID : null;
    },
    load(id: string) {
        if (id !== LIQI_METHODS_ID) return null;
        const proto = readFileSync(resolve(__dirname, "../proto/liqi.proto"), "utf8");
        const packageName = proto.match(/^\s*package\s+([\w.]+)\s*;/m)?.[1] ?? "lq";
        const methods = Array.from(proto.matchAll(/^\s*service\s+(\w+)\s*\{([\s\S]*?)^\}/gm))
            .flatMap((service) => Array.from(service[2].matchAll(/^\s*rpc\s+(\w+)\s*\(/gm), (rpc) => `.${packageName}.${service[1]}.${rpc[1]}`));
        if (!methods.length) throw new Error("No RPC methods found in proto/liqi.proto");
        return `export default ${JSON.stringify(methods)}`;
    },
});

export default defineConfig({
    define: {
        __APP_VERSION__: JSON.stringify(APP_VERSION),
    },
    resolve: {
        dedupe: ["react", "react-dom"],
    },
    plugins: [liqiMethods(), react({fastRefresh: false})],
    server: {
        port: 5173,
        strictPort: true,
        watch: {ignored: ['**/src-tauri/target/**']},
    },
    clearScreen: false,
    build: {
        rollupOptions: {
            input: {
                index: resolve(__dirname, "index.html"),
                settings: resolve(__dirname, "settings.html"),
                msgbox: resolve(__dirname, "msgbox.html"),
                overlay: resolve(__dirname, "overlay.html"),
                packetViewer: resolve(__dirname, "packet-viewer.html"),
            },
        },
    }
})
