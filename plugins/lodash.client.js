import _ from 'lodash'

export default defineNuxtPlugin((nuxtApp) => {
    nuxtApp.provide('_', _)
})
