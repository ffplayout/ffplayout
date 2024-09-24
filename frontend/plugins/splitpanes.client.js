import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.vueApp.component('Splitpanes', Splitpanes)
    nuxtApp.vueApp.component('Pane', Pane)
})
