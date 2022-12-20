export default {
    server: {
        port: 3000, // default: 3000
        host: '127.0.0.1' // default: localhost
    },

    ssr: false,
    /*
     ** Headers of the page
     */
    head: {
        title: process.env.npm_package_name || '',
        meta: [
            {
                charset: 'utf-8'
            },
            {
                name: 'viewport',
                content: 'width=device-width, initial-scale=1'
            },
            {
                hid: 'description',
                name: 'description',
                content: process.env.npm_package_description || ''
            }
        ],
        link: [
            {
                rel: 'icon',
                type: 'image/x-icon',
                href: '/favicon.ico'
            }
        ]
    },
    /*
     ** Customize the progress-bar color
     */
    loading: {
        color: '#ff9c36'
    },
    /*
     ** Global CSS
     */
    css: ['@/assets/css/bootstrap.min.css'],
    /*
     ** Plugins to load before mounting the App
     */
    plugins: [
        { src: '~/plugins/axios' },
        { src: '~/plugins/filters' },
        { src: '~/plugins/helpers.js' },
        { src: '~/plugins/nuxt-client-init.js', ssr: false },
        { src: '~plugins/video.js', ssr: false },
        { src: '~plugins/splitpanes.js', ssr: false },
        { src: '~plugins/loading.js', ssr: false },
        { src: '~plugins/draggable.js', ssr: false },
        { src: '~plugins/lodash.js', ssr: false }
    ],
    /*
     ** Nuxt.js modules
     */
    modules: [
        // Doc: https://bootstrap-vue.js.org
        'bootstrap-vue/nuxt',
        '@nuxtjs/axios',
        '@nuxtjs/dayjs',
        '@nuxtjs/eslint-module',
        '@nuxtjs/style-resources',
        'cookie-universal-nuxt'
    ],

    /*
     ** Axios module configuration
     ** See https://axios.nuxtjs.org/options
     */
    axios: {
        baseURL: '/'
    },

    dayjs: {
        locales: ['en', 'de'],
        defaultLocale: 'en',
        defaultTimeZone: 'UTC',
        plugins: ['utc', 'timezone']
    },

    styleResources: {
        scss: ['@/assets/css/_variables.scss', '@/assets/scss/globals.scss']
    },

    bootstrapVue: {
        bootstrapCSS: false,
        icons: true
    },

    /*
     ** Build configuration
     */
    build: {
        /*
         ** You can extend webpack config here
         */
        extend (config, ctx) {},
        babel: { compact: true },
        loaders: {
            sass: {
                implementation: require('sass')
            },
            scss: {
                implementation: require('sass')
            }
        }
    }
}
