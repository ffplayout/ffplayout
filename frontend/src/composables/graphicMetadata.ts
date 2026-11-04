import { shallowRef, watch } from 'vue'
import { authFetch } from '@/composables/authFetch'

export interface GraphicMetadata {
    width: number | null
    height: number | null
    source_duration: number | null
}

/** Inspect only the selected source; discard outdated results after edits/channel changes. */
export function useGraphicMetadata(channel: () => number | undefined, source: () => string | undefined) {
    const info = shallowRef<GraphicMetadata>()
    watch(
        [channel, source],
        ([id, path], _, onCleanup) => {
            info.value = undefined
            if (!id || !path?.trim()) return
            const controller = new AbortController()
            const timer = setTimeout(async () => {
                try {
                    const result = await authFetch<GraphicMetadata>(`/api/file/${id}/media-info`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ source: path.trim() }),
                        signal: controller.signal,
                    })
                    if (!controller.signal.aborted) info.value = result
                } catch {
                    // Optional metadata must not prevent editing an unavailable source.
                }
            }, 250)
            onCleanup(() => {
                clearTimeout(timer)
                controller.abort()
            })
        },
        { immediate: true },
    )
    return info
}
