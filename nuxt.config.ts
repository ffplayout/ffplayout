// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
    devServer: {
        port: 3000, // default: 3000
        host: '127.0.0.1', // default: localhost
    },

    nitro: {
        devProxy: {
            '/api': { target: 'http://127.0.0.1:8787/api' },
            '/auth': { target: 'http://127.0.0.1:8787/auth' },
            '/live': { target: 'http://127.0.0.1:8787/live' },
            '/file': { target: 'http://127.0.0.1:8787/file' },
        },
    },

    ignore: ['**/public/tv-media**', '**/public/Videos**', '**/public/live**', '**/public/home**'],
    ssr: false,
    // debug: true,

    app: {
        head: {
            title: 'ffplayout',
            meta: [
                {
                    charset: 'utf-8',
                },
                {
                    name: 'viewport',
                    content: 'width=device-width, initial-scale=1',
                },
                {
                    hid: 'description',
                    name: 'description',
                    content: 'Frontend for ffplayout, the 24/7 Rust and playlist based streaming solution.',
                },
            ],
            link: [
                {
                    rel: 'icon',
                    type: 'image/x-icon',
                    href: '/favicon.ico',
                },
            ],
        },
    },

    modules: ['@nuxtjs/color-mode', '@pinia/nuxt', '@vueuse/nuxt', '@nuxtjs/tailwindcss'],
    css: ['@/assets/scss/main.scss'],

    colorMode: {
        preference: 'dark', // default value of $colorMode.preference
        fallback: 'system', // fallback value if not system preference found
        hid: 'nuxt-color-mode-script',
        globalName: '__NUXT_COLOR_MODE__',
        componentName: 'ColorScheme',
        classPrefix: '',
        classSuffix: '',
        dataValue: 'theme',
        storageKey: 'theme',
    },

    vite: {
        build: {
            chunkSizeWarningLimit: 800000,
        },
    },

    experimental: {
        payloadExtraction: false,
    },
})
