import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'

declare module '#app' {
    interface NuxtApp {
        $dayjs(date?: dayjs.ConfigType): dayjs.Dayjs
    }
}
declare module '@vue/runtime-core' {
    interface ComponentCustomProperties {
        $dayjs(date?: dayjs.ConfigType): dayjs.Dayjs
    }
}

export default defineNuxtPlugin((nuxtApp) => {
    dayjs.extend(customParseFormat)
    dayjs.extend(timezone)
    dayjs.extend(utc)

    nuxtApp.provide('dayjs', dayjs)
})
