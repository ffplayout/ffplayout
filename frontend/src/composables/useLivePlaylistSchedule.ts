import { computed, onBeforeUnmount, ref, shallowRef, watch } from 'vue'
import { authFetchRaw } from '@/composables/authFetch'
import { broadcastPosition, scheduledClipAt, type ScheduledClip } from '@/composables/livePlaylistSchedule'

export function useLivePlaylistSchedule(
    options: () => {
        active: boolean
        channel: number | undefined
        timezone: string
        start: number
        length: number
        infinite: boolean
        shift: number
    },
) {
    const tick = ref(Date.now())
    const serverOffset = ref(0)
    const program = shallowRef<ScheduledClip[]>([])
    const loadedDate = ref('')
    const schedule = computed(() => {
        const config = options()
        return broadcastPosition(tick.value + serverOffset.value, config.timezone, config.start, config.shift)
    })
    const current = computed(() => {
        const config = options()
        if (!config.active || loadedDate.value !== schedule.value.date) return undefined
        if (!config.channel) return undefined
        const position = scheduledClipAt(program.value, schedule.value.elapsed, config.length, config.infinite)
        return position ? { ...position, channel: config.channel, date: loadedDate.value } : undefined
    })
    let request: AbortController | undefined
    let lastFetch = 0
    async function refresh() {
        const config = options()
        if (!config.active || !config.channel || request) return
        const controller = new AbortController()
        request = controller
        const date = schedule.value.date
        lastFetch = Date.now()
        try {
            const response = await authFetchRaw(`/api/playlist/${config.channel}?date=${date}`, {
                signal: controller.signal,
                cache: 'no-store',
            })
            if (!response.ok) throw new Error('Playlist unavailable')
            const data = (await response.json()) as { program?: ScheduledClip[] }
            if (controller.signal.aborted) return
            const serverDate = Date.parse(response.headers.get('Date') ?? '')
            if (Number.isFinite(serverDate)) serverOffset.value = serverDate - Date.now()
            program.value = data.program ?? []
            loadedDate.value = date
        } catch {
            if (!controller.signal.aborted) {
                program.value = []
                loadedDate.value = ''
            }
        } finally {
            if (request === controller) request = undefined
        }
    }
    watch(
        [
            () => options().active,
            () => options().channel,
            () => options().timezone,
            () => options().start,
            () => schedule.value.date,
        ],
        () => {
            request?.abort()
            request = undefined
            program.value = []
            loadedDate.value = ''
            void refresh()
        },
        { immediate: true },
    )
    const timer = setInterval(() => {
        tick.value = Date.now()
        if (Date.now() - lastFetch >= 30_000) void refresh()
    }, 1000)
    onBeforeUnmount(() => {
        clearInterval(timer)
        request?.abort()
    })
    return current
}
