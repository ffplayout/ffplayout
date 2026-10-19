import type { Broadcast } from '@moq/net'
import type { Effect, Getter } from '@moq/signals'

export interface MoqMeterClient {
    signals: Effect
    broadcast: { out: { active: Getter<Broadcast.Consumer | undefined> } }
}

export type MoqLoudnessMetrics = Pick<
    LiveLoudnessMetrics,
    'momentary_lufs' | 'short_term_lufs' | 'integrated_lufs' | 'true_peak_dbtp'
>

export interface MoqAudioLevel extends AudioLevel {
    timestamp_us?: number | null
}

export function subscribeMoqAudioLevel(player: MoqMeterClient, update: (level: MoqAudioLevel | null) => void) {
    return subscribeMeter(player, 'audio-level.json', (value) => {
        if (
            value &&
            typeof value === 'object' &&
            'peak_db' in value &&
            'rms_db' in value &&
            typeof value.peak_db === 'number' &&
            Number.isFinite(value.peak_db) &&
            typeof value.rms_db === 'number' &&
            Number.isFinite(value.rms_db)
        ) {
            const level: MoqAudioLevel = { peak_db: value.peak_db, rms_db: value.rms_db }
            if (
                'timestamp_us' in value &&
                typeof value.timestamp_us === 'number' &&
                Number.isSafeInteger(value.timestamp_us) &&
                value.timestamp_us >= 0
            ) {
                level.timestamp_us = value.timestamp_us
            }
            update(level)
        } else {
            update(null)
        }
    })
}

export function subscribeMoqLoudness(player: MoqMeterClient, update: (level: MoqLoudnessMetrics | null) => void) {
    return subscribeMeter(player, 'audio-loudness.json', (value) => {
        const keys = ['momentary_lufs', 'short_term_lufs', 'integrated_lufs', 'true_peak_dbtp'] as const
        if (!value || typeof value !== 'object') {
            update(null)
            return
        }
        const metrics = value as Record<string, unknown>
        if (
            keys.every(
                (key) => metrics[key] === null || (typeof metrics[key] === 'number' && Number.isFinite(metrics[key])),
            )
        ) {
            update(metrics as MoqLoudnessMetrics)
        } else {
            update(null)
        }
    })
}

/** Every group contains one current reading; cleanup releases the network subscription. */
function subscribeMeter(player: MoqMeterClient, name: string, update: (value: unknown) => void) {
    return player.signals.run((effect) => {
        const broadcast = effect.get(player.broadcast.out.active)
        update(null)
        if (!broadcast) return

        const track = broadcast.track(name).subscribe({ priority: 10 })
        let closed = false
        let lastReceived = 0
        let closeGroup: (() => void) | undefined
        effect.cleanup(() => {
            closed = true
            closeGroup?.()
            track.close()
            update(null)
        })
        // Clear a stalled meter rather than displaying the last peak indefinitely.
        effect.interval(() => {
            if (performance.now() - lastReceived > 1000) update(null)
        }, 250)
        effect.spawn(async () => {
            try {
                while (!closed) {
                    const group = await track.recvGroup()
                    if (!group) break
                    closeGroup = () => group.close()
                    try {
                        const value: unknown = await group.readJson()
                        if (closed) return
                        lastReceived = performance.now()
                        update(value)
                    } catch {
                        // A late group can expire while being read; the next is independent.
                        if (!closed) update(null)
                    } finally {
                        group.close()
                        closeGroup = undefined
                    }
                }
            } catch {
                // Reconnection replaces the broadcast and creates a fresh subscription.
            } finally {
                track.close()
                if (!closed) update(null)
            }
        })
    })
}
