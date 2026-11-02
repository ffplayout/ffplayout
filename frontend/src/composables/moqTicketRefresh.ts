import type { Effect, Getter } from '@moq/signals'

function retryable(reason: unknown): boolean {
    if (reason && typeof reason === 'object' && 'response' in reason && reason.response instanceof Response) {
        return ![400, 401, 403, 404].includes(reason.response.status)
    }
    return true
}

/** A private ticket is single-use: obtain a new URL as soon as its session drops. */
export function refreshMoqTicketOnDisconnect(
    signals: Effect,
    connection: {
        url: Getter<URL | undefined>
        status: Getter<'connecting' | 'connected' | 'disconnected'>
        error: Getter<Error | undefined>
    },
    refresh: (signal: AbortSignal) => Promise<void>,
) {
    let firstUrl = true
    return signals.run((scope) => {
        if (!scope.get(connection.url)) return

        const initialUrl = firstUrl
        firstUrl = false
        const inheritedFailure = connection.error.peek()
        let attempted = false
        let controller: AbortController | undefined
        let timer: ReturnType<typeof setTimeout> | undefined
        let active = false
        let denied = false
        let closed = false
        let delay = 500
        function cancel() {
            clearTimeout(timer)
            timer = undefined
            controller?.abort()
            controller = undefined
            active = false
        }
        scope.cleanup(() => {
            closed = true
            cancel()
        })
        async function attempt() {
            timer = undefined
            const request = new AbortController()
            controller = request
            try {
                // The callback installs a fresh URL and checks this signal
                // before doing so. URL changes dispose this entire recovery.
                await refresh(request.signal)
            } catch (reason) {
                if (closed || request.signal.aborted) return
                if (!retryable(reason)) {
                    denied = true
                    active = false
                    return
                }
                timer = setTimeout(() => void attempt(), delay)
                delay = Math.min(delay * 2, 5000)
            }
        }
        scope.run((state) => {
            const status = state.get(connection.status)
            const failure = state.get(connection.error)
            if (status === 'connecting') {
                attempted = true
                return
            }
            if (status === 'connected' && !failure) {
                attempted = true
                cancel()
                denied = false
                delay = 500
            } else if (
                ((status === 'disconnected' && attempted) ||
                    (failure && (initialUrl || failure !== inheritedFailure))) &&
                !active &&
                !denied
            ) {
                active = true
                timer = setTimeout(() => void attempt(), 0)
            }
            // A new URL can briefly expose the old disconnected/error state.
            // Let it attempt a handshake before renewing that fresh ticket.
            // The library may start another attempt meanwhile. Do not cancel
            // a slow credential fetch on a transition back to "connecting".
        })
    })
}
