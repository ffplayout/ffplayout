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
        },
    },

    ignore: ['**/public/tv-media**', '**/public/Videos**', '**/public/live**', '**/public/home**'],

    ssr: false,

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

    modules: ['@pinia/nuxt'],

    vite: {
        css: {
            preprocessorOptions: {
                scss: {
                    additionalData: '@import "@/assets/scss/main.scss";',
                },
            },
        },
    },
})
