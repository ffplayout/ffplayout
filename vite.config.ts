import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
export default defineConfig({
    build: {
        chunkSizeWarningLimit: 1600,
    },
    root: './frontend',
    plugins: [tailwindcss(), vue(), vueDevTools()],
    resolve: {
        alias: {
            '@': fileURLToPath(new URL('./frontend/src', import.meta.url)),
        },
    },
    server: {
        host: '127.0.0.1',
        port: 5757,
        proxy: {
            '/api': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/auth': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/data': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/live': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/1/live': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/file': { target: 'http://127.0.0.1:8787', changeOrigin: true },
            '/hls': { target: 'http://127.0.0.1:8787', changeOrigin: true },
        },
    },
})
