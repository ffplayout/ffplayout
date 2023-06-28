import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

declare module '#app' {
	interface NuxtApp {
		$dayjs(date?: dayjs.ConfigType): dayjs.Dayjs;
	}
}
declare module '@vue/runtime-core' {
	interface ComponentCustomProperties {
		$dayjs(date?: dayjs.ConfigType): dayjs.Dayjs;
	}
}

export default defineNuxtPlugin((nuxtApp) => {
    dayjs.extend(utc)
    dayjs.extend(timezone)
    nuxtApp.provide('dayjs', dayjs)
})
