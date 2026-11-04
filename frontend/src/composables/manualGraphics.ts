import { computed, ref } from 'vue'
import type { GraphicEvent } from './playlistGraphics'

export interface ManualGraphicLayer {
    layer: number
    name: string
    settings: GraphicEvent
    running: boolean
    visible: boolean
    active: boolean
    error?: string
}

export interface ManualGraphicsStatus {
    revision: number
    running: boolean
    canvas: [number, number]
    layers: ManualGraphicLayer[]
}

/** Network state is kept separate from editable layer drafts. */
export function useManualGraphicsControl(
    channel: () => number | undefined,
    request: <T>(url: string, options?: RequestInit) => Promise<T>,
) {
    const status = ref<ManualGraphicsStatus>()
    const busy = ref(false)
    const pollError = ref('')
    const commandError = ref('')
    const error = computed(() => commandError.value || pollError.value)
    let generation = 0
    let poll: AbortController | undefined

    async function refresh() {
        const id = channel()
        if (!id || busy.value || poll) return
        const controller = new AbortController()
        poll = controller
        const version = generation
        try {
            const result = await request<ManualGraphicsStatus>(`/api/graphics/${id}`, { signal: controller.signal })
            if (generation === version && !controller.signal.aborted) {
                status.value = result
                pollError.value = ''
            }
        } catch (reason) {
            if (!controller.signal.aborted && generation === version) {
                // Keep mounted layer editors and their unsaved drafts on a transient failure.
                pollError.value = reason instanceof Error ? reason.message : String(reason)
            }
        } finally {
            if (poll === controller) poll = undefined
        }
    }

    async function command(data: object): Promise<boolean> {
        const id = channel()
        if (!status.value || busy.value || !id) return false
        poll?.abort()
        poll = undefined
        const version = generation
        const revision = status.value.revision
        busy.value = true
        commandError.value = ''
        pollError.value = ''
        let succeeded = false
        try {
            await request(`/api/graphics/${id}`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ ...data, revision }),
            })
            succeeded = true
        } catch (reason) {
            if (version === generation) commandError.value = reason instanceof Error ? reason.message : String(reason)
        } finally {
            if (version === generation) {
                busy.value = false
                await refresh()
            }
        }
        return succeeded && version === generation
    }

    function reset() {
        generation++
        poll?.abort()
        poll = undefined
        busy.value = false
        status.value = undefined
        commandError.value = ''
        pollError.value = ''
    }

    return { status, busy, error, refresh, command, reset }
}
