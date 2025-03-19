import { defineNuxtPlugin } from '#imports'
import Multiselect from '@vueform/multiselect'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.vueApp.component('Multiselect', Multiselect)
})
