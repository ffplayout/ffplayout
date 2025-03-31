import './assets/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import { createHead } from '@unhead/vue/client'

import i18nInstance from './i18n.ts'

import App from './App.vue'
import router from './router/index.ts'

const app = createApp(App)

const head = createHead({
    init: [
        {
            title: 'System',
            titleTemplate: '%s | ffplayout',
            htmlAttrs: { lang: 'en' },
        },
    ],
})

app.use(i18nInstance)
app.use(head)
app.use(createPinia())
app.use(router)

app.mount('#app')
