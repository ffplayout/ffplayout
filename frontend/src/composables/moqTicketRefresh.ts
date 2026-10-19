import type { Effect, Getter } from '@moq/signals'

function retryable(reason: unknown): boolean {
    if (reason && typeof reason === 'object' && 'response' in reason && reason.response instanceof Response) {
        return ![400, 401, 403, 404].includes(reason.response.status)
    }
    return true
}

/** Refresh credentials only after the library has stopped reconnecting this URL. */
export function refreshMoqTicketOnFailure(
    signals: Effect,
    connection: { url: Getter<URL | undefined>; error: Getter<Error | undefined> },
    refresh: (signal: AbortSignal) => Promise<void>,
) {
    return signals.run((effect) => {
        const url = effect.get(connection.url)
        const failure = effect.get(connection.error)
        if (!url || !failure) return

        const controller = new AbortController()
        effect.cleanup(() => controller.abort())
        let delay = 2000
        async function attempt() {
            try {
                await refresh(controller.signal)
            } catch (reason) {
                if (controller.signal.aborted || !retryable(reason)) return
                // A temporary bootstrap outage must not strand the terminal URL.
                // Retry serially, with a bounded backoff, until access is restored.
                delay = Math.min(delay * 2, 30_000)
                effect.timer(() => {
                    void attempt()
                }, delay)
            }
        }
        // Recovery, a changed URL or unmount cancels both waits and in-flight fetches.
        // Transient media disconnects belong to the library's own reconnect loop.
        effect.timer(() => {
            void attempt()
        }, delay)
    })
}
