import './assets/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import i18nInstance from './i18n.ts'

import App from './App.vue'
import router from './router/index.ts'

const app = createApp(App)

app.use(i18nInstance)
app.use(createPinia())
app.use(router)

app.mount('#app')
