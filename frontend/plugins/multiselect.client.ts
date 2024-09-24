import { defineNuxtPlugin } from '#imports'
import Multiselect from '@vueform/multiselect'
import '@vueform/multiselect/themes/tailwind.css'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.vueApp.component('Multiselect', Multiselect)
})
