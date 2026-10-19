import { shallowRef } from 'vue'
import type MoqWatch from '@moq/watch/element'

export function moqDiagnostics(player: MoqWatch) {
    const snapshot = shallowRef<unknown>()
    // Count each pipeline stage separately: the upstream frameCount is
    // incremented before decode and does not count displayed frames.
    const history: unknown[] = []
    let received = 0
    let scheduled = 0
    let rendered = 0
    let maxGap = 0
    let maxReceivedGap = 0
    let maxScheduledGap = 0
    let lastReceived: number | undefined
    let lastScheduled: number | undefined
    let lastRender: number | undefined
    let lastSample = performance.now()
    const started = lastSample
    player.signals.run((effect) => {
        const stats = effect.get(player.video.out.stats)
        if (!stats || player.paused || document.hidden) {
            lastReceived = undefined
            return
        }
        const now = performance.now()
        if (lastReceived !== undefined) maxReceivedGap = Math.max(maxReceivedGap, now - lastReceived)
        lastReceived = now
    })
    player.signals.run((effect) => {
        const timestamp = effect.get(player.video.out.timestamp)
        if (timestamp === undefined || player.paused || document.hidden) {
            lastScheduled = undefined
            return
        }
        const now = performance.now()
        if (lastScheduled !== undefined) maxScheduledGap = Math.max(maxScheduledGap, now - lastScheduled)
        lastScheduled = now
        scheduled++
    })
    player.signals.run((effect) => {
        const timestamp = effect.get(player.renderer.out.timestamp)
        if (timestamp === undefined || player.paused || document.hidden) {
            lastRender = undefined
            return
        }
        const now = performance.now()
        if (lastRender !== undefined) maxGap = Math.max(maxGap, now - lastRender)
        lastRender = now
        rendered++
    })
    const measure = setInterval(() => {
        const now = performance.now()
        const seconds = (now - lastSample) / 1000
        const playing = !player.paused && Boolean(player.url)
        if (!playing || document.hidden) lastReceived = lastScheduled = lastRender = undefined
        if (lastReceived !== undefined) maxReceivedGap = Math.max(maxReceivedGap, now - lastReceived)
        if (lastScheduled !== undefined) maxScheduledGap = Math.max(maxScheduledGap, now - lastScheduled)
        if (lastRender !== undefined) maxGap = Math.max(maxGap, now - lastRender)
        const count = player.video.out.stats.peek()?.frameCount ?? 0
        const video = player.renderer.out.timestamp.peek()
        const audio = player.muted ? undefined : player.audio.out.timestamp.peek()
        const clock = player.sync.now()
        const finite = (value: number | undefined) => (value !== undefined && Number.isFinite(value) ? Math.round(value) : null)
        const sample = {
            elapsed_s: Math.round((now - started) / 1000),
            playing,
            visible: !document.hidden,
            transport: player.connection.transport.peek(),
            delay_ms: finite(player.sync.out.delay.peek()),
            received_fps: Math.max(0, count - received) / seconds,
            scheduled_fps: scheduled / seconds,
            rendered_fps: rendered / seconds,
            max_received_gap_ms: finite(maxReceivedGap),
            max_scheduled_gap_ms: finite(maxScheduledGap),
            max_render_gap_ms: finite(maxGap),
            video_behind_clock_ms: finite(
                !playing || clock === undefined || video === undefined ? undefined : clock - video,
            ),
            audio_ahead_video_ms: finite(
                !playing || audio === undefined || video === undefined ? undefined : audio - video,
            ),
            video_stalled: player.video.out.stalled.peek(),
            audio_stalled: player.muted ? null : player.audio.out.stalled.peek(),
        }
        history.push(sample)
        if (history.length > 120) history.shift()
        snapshot.value = sample
        received = count
        scheduled = rendered = maxGap = maxReceivedGap = maxScheduledGap = 0
        lastSample = now
    }, 1000)
    return {
        snapshot,
        report: () => JSON.stringify({ browser: navigator.userAgent, crossOriginIsolated, samples: history }, null, 2),
        close: () => clearInterval(measure),
    }
}
