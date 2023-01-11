import { defineNuxtPlugin } from '#app'
import { Sortable } from 'sortablejs-vue3'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.vueApp.component('Sortable', Sortable)
})
