import { defineNuxtPlugin } from '#imports'
import VirtualList from 'vue-virtual-draglist'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.vueApp.component('VirtualList', VirtualList)
})
