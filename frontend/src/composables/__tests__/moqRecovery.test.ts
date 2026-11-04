import assert from 'node:assert/strict'
import { test, type TestContext } from 'node:test'
import { Effect, Signal } from '@moq/signals'
import { refreshMoqTicketOnDisconnect } from '../moqTicketRefresh.ts'
import { resumeMoqAudio } from '../moqAudioResume.ts'

async function flush() {
    for (let i = 0; i < 10; i++) await Promise.resolve()
}

async function tick(t: TestContext, milliseconds = 0) {
    await flush()
    t.mock.timers.tick(milliseconds)
    await flush()
}

function connection() {
    return {
        url: new Signal<URL | undefined>(new URL('https://relay.test/?ticket=used')),
        status: new Signal<'connected' | 'connecting' | 'disconnected'>('connected'),
        error: new Signal<Error | undefined>(undefined),
    }
}

test('a dropped session refreshes immediately, before the library gives up', async (t) => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const signals = new Effect()
    t.after(() => signals.close())
    const relay = connection()
    const requests: AbortSignal[] = []
    refreshMoqTicketOnDisconnect(signals, relay, async (signal) => {
        requests.push(signal)
        await new Promise<void>((resolve) => signal.addEventListener('abort', () => resolve(), { once: true }))
    })
    await tick(t)
    assert.equal(requests.length, 0, 'healthy connections must not refresh periodically')
    relay.status.set('disconnected')
    await tick(t)
    assert.equal(requests.length, 1)
    relay.status.set('connecting')
    await tick(t, 2000)
    assert.equal(requests.length, 1, 'slow bootstrap requests must not overlap')
    assert.equal(requests[0].aborted, false, 'a library retry must not cancel credential renewal')
    relay.url.set(new URL('https://relay.test/?ticket=fresh'))
    await flush()
    assert.equal(requests[0].aborted, true, 'replaced URLs must cancel their old request')
})

test('a freshly installed URL gets a handshake before another credential refresh', async (t) => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const signals = new Effect()
    t.after(() => signals.close())
    const relay = connection()
    let calls = 0
    refreshMoqTicketOnDisconnect(signals, relay, async () => {
        calls++
        relay.url.set(new URL(`https://relay.test/?ticket=fresh-${calls}`))
    })
    await flush()
    relay.status.set('disconnected')
    await tick(t)
    await tick(t, 30_000)
    assert.equal(calls, 1, 'stale disconnected state must not churn fresh tickets')
    relay.status.set('connecting')
    await flush()
    relay.status.set('disconnected')
    await tick(t)
    assert.equal(calls, 2, 'a rejected fresh handshake must renew again')
})

test('bootstrap outages retry serially with bounded backoff and stop on recovery', async (t) => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const signals = new Effect()
    t.after(() => signals.close())
    const relay = connection()
    let calls = 0
    refreshMoqTicketOnDisconnect(signals, relay, async () => {
        calls++
        throw new Error('temporary outage')
    })
    await flush()
    relay.status.set('disconnected')
    await tick(t)
    assert.equal(calls, 1)
    for (const delay of [500, 1000, 2000, 4000, 5000, 5000]) {
        const before = calls
        await tick(t, delay - 1)
        assert.equal(calls, before)
        await tick(t, 1)
        assert.equal(calls, before + 1)
    }
    relay.status.set('connected')
    await tick(t, 30_000)
    assert.equal(calls, 7)
    relay.status.set('disconnected')
    await tick(t)
    assert.equal(calls, 8, 'a later outage gets a fresh recovery')
    signals.close()
    await tick(t, 30_000)
    assert.equal(calls, 8)
})

test('denied bootstrap access is not retried and unmount aborts in-flight requests', async (t) => {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const signals = new Effect()
    t.after(() => signals.close())
    const relay = connection()
    let calls = 0
    let pending: AbortSignal | undefined
    refreshMoqTicketOnDisconnect(signals, relay, async (signal) => {
        calls++
        if (calls === 1) throw { response: new Response(null, { status: 403 }) }
        pending = signal
        await new Promise<void>((resolve) => signal.addEventListener('abort', () => resolve(), { once: true }))
    })
    relay.error.set(new Error('unauthorized'))
    await tick(t)
    await tick(t, 60_000)
    assert.equal(calls, 1)
    relay.url.set(new URL('https://relay.test/?ticket=renewed'))
    await flush()
    relay.status.set('connecting')
    relay.error.set(undefined)
    await flush()
    relay.status.set('disconnected')
    relay.error.set(new Error('new handshake rejected'))
    await tick(t)
    assert.equal(calls, 2)
    signals.close()
    assert.equal(pending?.aborted, true)
})

class TestAudioContext extends EventTarget {
    state: AudioContextState = 'suspended'
    calls = 0
    async resume() {
        this.calls++
        this.state = 'running'
    }
}

test('audio resumes on play and interruption while respecting pause/mute and cleanup', async () => {
    const signals = new Effect()
    const audio = new TestAudioContext()
    const context = new Signal<AudioContext | undefined>(audio as unknown as AudioContext)
    const controls = { paused: new Signal(true), muted: new Signal(false) }
    const document = Object.assign(new EventTarget(), { visibilityState: 'visible' }) as unknown as Document
    const window = new EventTarget() as Window
    resumeMoqAudio(signals, context, controls, document, window)
    try {
        await flush()
        assert.equal(audio.calls, 0)
        controls.paused.set(false)
        await flush()
        assert.equal(audio.calls, 1)
        audio.state = 'suspended'
        audio.dispatchEvent(new Event('statechange'))
        await flush()
        assert.equal(audio.calls, 2)
        controls.muted.set(true)
        await flush()
        audio.state = 'suspended'
        window.dispatchEvent(new Event('pageshow'))
        assert.equal(audio.calls, 2)
        controls.muted.set(false)
        await flush()
        assert.equal(audio.calls, 3)
        audio.state = 'suspended'
        document.dispatchEvent(new Event('visibilitychange'))
        await flush()
        assert.equal(audio.calls, 4)
        const replacement = new TestAudioContext()
        context.set(replacement as unknown as AudioContext)
        await flush()
        assert.equal(replacement.calls, 1)
        signals.close()
        replacement.state = 'suspended'
        document.dispatchEvent(new Event('pointerup'))
        window.dispatchEvent(new Event('pageshow'))
        assert.equal(replacement.calls, 1)
    } finally {
        signals.close()
    }
})

test('a pending automatic audio resume does not suppress a real user gesture', async () => {
    const signals = new Effect()
    const audio = new TestAudioContext()
    const finish: Array<() => void> = []
    audio.resume = () => {
        audio.calls++
        return new Promise<void>((resolve) => finish.push(resolve))
    }
    const document = Object.assign(new EventTarget(), { visibilityState: 'visible' }) as unknown as Document
    const window = new EventTarget() as Window
    resumeMoqAudio(
        signals,
        new Signal(audio as unknown as AudioContext),
        { paused: new Signal(false), muted: new Signal(false) },
        document,
        window,
    )
    try {
        await flush()
        assert.equal(audio.calls, 1)
        document.dispatchEvent(new Event('pointerup'))
        assert.equal(audio.calls, 2)
        audio.state = 'running'
        for (const resolve of finish) resolve()
        await flush()
        document.dispatchEvent(new Event('pointerup'))
        assert.equal(audio.calls, 2)
    } finally {
        signals.close()
    }
})
