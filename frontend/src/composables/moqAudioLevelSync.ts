import type { Getter } from '@moq/signals'
import { subscribeMoqAudioLevel, type MoqMeterClient } from './moqAudioLevel.ts'

interface TimedLevel extends AudioLevel {
    timestamp_us: number
}

// Measurements cover 250 ms and carry the window's end timestamp. Align the
// display to its midpoint instead of adding a full window of visual delay.
const MEASUREMENT_HALF_WINDOW_MS = 125

interface PlaybackMeterClient extends MoqMeterClient {
    controls: { paused: Getter<boolean>; muted: Getter<boolean> }
    renderer: { out: { timestamp: Getter<number | undefined> } }
    audio: {
        out: {
            timestamp: Getter<number | undefined>
            stalled: Getter<boolean>
            context: Getter<AudioContext | undefined>
        }
    }
}

/** Bounded even during a long pause or a stalled/hidden browser tab. */
export class AudioLevelTimeline {
    private readings: TimedLevel[] = []
    private current: TimedLevel | null = null
    private previousPosition: number | undefined

    push(level: TimedLevel) {
        const last = this.readings.at(-1)
        if (last && level.timestamp_us < last.timestamp_us) this.clear()
        if (last?.timestamp_us === level.timestamp_us) this.readings.pop()
        this.readings.push(level)
        if (this.readings.length > 40) this.readings.shift()
    }

    at(positionMs: number): AudioLevel | null {
        if (this.previousPosition !== undefined && positionMs < this.previousPosition - 100) {
            this.clear()
            return null
        }
        this.previousPosition = positionMs
        while (this.readings.length && this.readings[0]!.timestamp_us <= positionMs * 1000) {
            this.current = this.readings.shift()!
        }
        // No extrapolation across a missing interval or a jump to a new live edge.
        if (this.current && positionMs * 1000 - this.current.timestamp_us > 1000000) this.current = null
        return this.current
    }

    clear() {
        this.readings = []
        this.current = null
        this.previousPosition = undefined
    }
}

/** Schedule server measurements against the decoded audio ring's actual read position. */
export function synchronizeMoqAudioLevel(player: PlaybackMeterClient, update: (level: AudioLevel | null) => void) {
    return player.signals.run((effect) => {
        const broadcast = effect.get(player.broadcast.out.active)
        const timeline = new AudioLevelTimeline()
        let latest: AudioLevel | null = null
        let latestReceived = 0
        let previousPosition: number | undefined
        let positionChanged = Date.now()
        let displayed: AudioLevel | null = null
        const show = (level: AudioLevel | null) => {
            if (level === displayed) return
            displayed = level
            update(level)
        }
        update(null)
        if (!broadcast) return
        effect.cleanup(() => {
            timeline.clear()
            update(null)
        })
        // The child subscription belongs to this broadcast lifetime, including reconnects.
        subscribeMoqAudioLevel({ signals: effect, broadcast: player.broadcast }, (level) => {
            latest = level
            latestReceived = Date.now()
            if (level && level.timestamp_us !== undefined && level.timestamp_us !== null) {
                timeline.push({ ...level, timestamp_us: level.timestamp_us })
            }
            // A metadata stall must not discard readings for audio still buffered
            // in the player. The media position expires them; disconnect clears all.
        })
        effect.interval(() => {
            const currentLevel = Date.now() - latestReceived <= 1000 ? latest : null
            const followPosition = (position: number) => {
                if (position !== previousPosition) {
                    previousPosition = position
                    positionChanged = Date.now()
                }
                // A frozen playhead must not freeze the program meter as well.
                show(Date.now() - positionChanged >= 500
                    ? currentLevel
                    : timeline.at(position + MEASUREMENT_HALF_WINDOW_MS))
            }
            if (player.controls.paused.peek()) {
                show(currentLevel)
                return
            }
            // Muting deliberately stops audio downloads in @moq/watch. Follow the
            // displayed video then, without enabling a decoder just for the meter.
            if (player.controls.muted.peek()) {
                const timestamp = player.renderer.out.timestamp.peek()
                if (timestamp === undefined) show(currentLevel)
                else followPosition(timestamp)
                return
            }
            const context = player.audio.out.context.peek()
            const timestamp = player.audio.out.timestamp.peek()
            if (!context || context.state !== 'running' || timestamp === undefined || player.audio.out.stalled.peek()) {
                show(currentLevel)
                return
            }
            // The worklet read position precedes the hardware output. Account for the
            // browser's reported output delay, without inventing a wall-clock playhead.
            const outputTime = context.getOutputTimestamp?.().contextTime
            const latencyMs =
                outputTime !== undefined && outputTime > 0
                    ? Math.max(0, (context.currentTime - outputTime) * 1000)
                    : Math.max(0, ((context.baseLatency || 0) + (context.outputLatency || 0)) * 1000)
            followPosition(timestamp - latencyMs)
        }, 50)
    })
}
