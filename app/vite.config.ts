import {defineConfig} from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from "path";

export default defineConfig({
    plugins: [react()],
    server: {port: 5173, strictPort: true},
    clearScreen: false,
    build: {
        rollupOptions: {
            input: {
                index: resolve(__dirname, "index.html"),
                settings: resolve(__dirname, "settings.html"),
            },
        },
    }
})