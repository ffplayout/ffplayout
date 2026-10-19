import { ref, watch, type Ref } from 'vue'
import { Effect, Signal } from '@moq/signals'
import { authFetch } from '@/composables/authFetch'
import { moqRelayUrl, type MoqBootstrap } from '@/composables/moqConnection'
import { subscribeMoqAudioLevel, subscribeMoqLoudness, type MoqLoudnessMetrics } from '@/composables/moqAudioLevel'
import { refreshMoqTicketOnFailure } from '@/composables/moqTicketRefresh'

/** Subscribe to meters without constructing a media player or requesting A/V tracks. */
export function useMoqAudioMeters(channel: Ref<number | undefined>, enabled: Ref<boolean>) {
    const audioLevel = ref<AudioLevel | null>(null)
    const loudness = ref<MoqLoudnessMetrics | null>(null)

    watch(
        [channel, enabled],
        async ([id, active], _, onCleanup) => {
            const signals = new Effect()
            const controller = new AbortController()
            let disposed = false
            audioLevel.value = null
            loudness.value = null
            onCleanup(() => {
                disposed = true
                controller.abort()
                signals.close()
                audioLevel.value = null
                loudness.value = null
            })
            if (!active || id === undefined) return

            try {
                const [config, { Connection, Path }, { Broadcast }] = await Promise.all([
                    authFetch<MoqBootstrap>(`/api/moq/config?channel=${id}`, { signal: controller.signal }),
                    import('@moq/net'),
                    import('@moq/watch'),
                ])
                if (disposed) return
                const connection = new Connection({ url: moqRelayUrl(config, id) })
                const name = new Signal(Path.from(config.broadcast))
                const broadcast = new Broadcast({ origin: connection.origin, name })
                signals.cleanup(() => {
                    broadcast.close()
                    connection.close()
                })
                const meters = { signals, broadcast }
                subscribeMoqAudioLevel(meters, (value) => {
                    audioLevel.value = value
                })
                subscribeMoqLoudness(meters, (value) => {
                    loudness.value = value
                })
                refreshMoqTicketOnFailure(signals, connection, async (signal) => {
                    const config = await authFetch<MoqBootstrap>(`/api/moq/config?channel=${id}`, { signal })
                    if (disposed || signal.aborted) return
                    name.set(Path.from(config.broadcast))
                    connection.url.set(moqRelayUrl(config, id))
                })
            } catch {
                // Unavailable broadcasts show empty meters; channel changes/unmount cancel setup.
                signals.close()
            }
        },
        { immediate: true },
    )

    return { audioLevel, loudness }
}
