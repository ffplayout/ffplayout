import type { Effect, Getter } from '@moq/signals'

/** Restore audio after tab/device interruptions without changing pause or mute. */
export function resumeMoqAudio(
    signals: Effect,
    context: Getter<AudioContext | undefined>,
    controls: { paused: Getter<boolean>; muted: Getter<boolean> },
    document: Document,
    window: Window,
) {
    return signals.run((effect) => {
        const audio = effect.get(context)
        const paused = effect.get(controls.paused)
        const muted = effect.get(controls.muted)
        if (!audio || paused || muted) return

        let pending = false
        const resume = (gesture = false) => {
            if ((!gesture && pending) || audio.state === 'running' || audio.state === 'closed') return
            pending = true
            void audio
                .resume()
                .catch(() => {})
                .finally(() => {
                    pending = false
                })
        }
        resume()
        effect.event(audio, 'statechange', () => resume())
        effect.event(window, 'pageshow', () => resume())
        effect.event(document, 'visibilitychange', () => {
            if (document.visibilityState === 'visible') resume()
        })
        // Browsers can leave an earlier resume() pending until activation.
        // A real gesture must still make its own resume() call in that case.
        effect.event(document, 'pointerup', () => resume(true))
        effect.event(document, 'keydown', () => resume(true))
    })
}
