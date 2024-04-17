import lodash from 'lodash'
import type { LoDashStatic } from 'lodash'

declare module '#app' {
    interface NuxtApp {
        $_: LoDashStatic
    }
}
declare module '@vue/runtime-core' {
    interface ComponentCustomProperties {
        $_: LoDashStatic
    }
}

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.provide('_', lodash)
})
