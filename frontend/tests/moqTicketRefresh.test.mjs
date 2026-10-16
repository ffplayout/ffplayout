import assert from 'node:assert/strict'
import { test } from 'node:test'
import { Effect, Signal } from '@moq/signals'
import { refreshMoqTicketOnFailure } from '../src/composables/moqTicketRefresh.ts'

// Signal effects settle through microtasks; no actual network or wall-clock wait.
const settle = async () => {
    for (let i = 0; i < 10; i++) await Promise.resolve()
}
function fixture(t, refresh) {
    t.mock.timers.enable({ apis: ['setTimeout'] })
    const signals = new Effect()
    const connection = { url: new Signal(undefined), error: new Signal(undefined) }
    let refreshes = 0
    refreshMoqTicketOnFailure(signals, connection, async (signal) => {
        refreshes++
        await refresh?.(refreshes, signal, connection)
    })
    t.after(() => signals.close())
    return { connection, signals, refreshes: () => refreshes }
}

test('a private connection becoming ready does not replace its ticket URL after two seconds', async (t) => {
    const f = fixture(t)
    const url = new URL('ws://localhost/moq/relay?channel=1&ticket=first')
    // URL assignment precedes connecting/connected status; it is not a terminal failure.
    f.connection.url.set(url)
    await settle()
    t.mock.timers.tick(2500)
    await settle()
    assert.equal(f.refreshes(), 0)
    assert.equal(f.connection.url.peek(), url)
})

test('a terminal connection failure requests one fresh ticket', async (t) => {
    const f = fixture(t)
    f.connection.url.set(new URL('ws://localhost/moq/relay?channel=1&ticket=first'))
    f.connection.error.set(new Error('credentials refused'))
    await settle()
    t.mock.timers.tick(2000)
    await settle()
    assert.equal(f.refreshes(), 1)
    t.mock.timers.tick(5000)
    await settle()
    assert.equal(f.refreshes(), 1)
})

test('recovery cancels a pending credential refresh', async (t) => {
    const f = fixture(t)
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=first'))
    f.connection.error.set(new Error('connection failed'))
    await settle()
    t.mock.timers.tick(1000)
    f.connection.error.set(undefined)
    await settle()
    t.mock.timers.tick(2000)
    await settle()
    assert.equal(f.refreshes(), 0)
})

test('unmount cancels a pending credential refresh', async (t) => {
    const f = fixture(t)
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=first'))
    f.connection.error.set(new Error('connection failed'))
    await settle()
    f.signals.close()
    t.mock.timers.tick(2500)
    await settle()
    assert.equal(f.refreshes(), 0)
})

test('temporary bootstrap failures back off and recover without another media error', async (t) => {
    const f = fixture(t, async (attempt, _signal, connection) => {
        if (attempt === 1) throw new TypeError('backend unreachable')
        if (attempt === 2)
            throw Object.assign(new Error('backend restarting'), { response: new Response(null, { status: 503 }) })
        connection.url.set(new URL('ws://localhost/moq/relay?ticket=fresh'))
        connection.error.set(undefined)
    })
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=old'))
    f.connection.error.set(new Error('credentials refused'))
    await settle()
    for (const [delay, attempts] of [
        [2000, 1],
        [4000, 2],
        [8000, 3],
    ]) {
        t.mock.timers.tick(delay)
        await settle()
        assert.equal(f.refreshes(), attempts)
    }
    assert.equal(f.connection.url.peek().searchParams.get('ticket'), 'fresh')
    t.mock.timers.tick(60000)
    await settle()
    assert.equal(f.refreshes(), 3)
})

test('permission denial stops bootstrap retries', async (t) => {
    const f = fixture(t, async () => {
        throw Object.assign(new Error('forbidden'), { response: new Response(null, { status: 403 }) })
    })
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=old'))
    f.connection.error.set(new Error('credentials refused'))
    await settle()
    t.mock.timers.tick(2000)
    await settle()
    t.mock.timers.tick(60000)
    await settle()
    assert.equal(f.refreshes(), 1)
})

test('recovery aborts an in-flight bootstrap and prevents another retry', async (t) => {
    let pendingSignal
    const f = fixture(t, async (_attempt, signal) => {
        pendingSignal = signal
        await new Promise((_resolve, reject) =>
            signal.addEventListener('abort', () => reject(new Error('aborted')), { once: true }),
        )
    })
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=old'))
    f.connection.error.set(new Error('connection failed'))
    await settle()
    t.mock.timers.tick(2000)
    await settle()
    assert.equal(f.refreshes(), 1)
    f.connection.error.set(undefined)
    await settle()
    assert.equal(pendingSignal.aborted, true)
    t.mock.timers.tick(60000)
    await settle()
    assert.equal(f.refreshes(), 1)
})

test('bootstrap retry backoff is capped at thirty seconds and cancelled on unmount', async (t) => {
    const f = fixture(t, async () => {
        throw new TypeError('backend unreachable')
    })
    f.connection.url.set(new URL('ws://localhost/moq/relay?ticket=old'))
    f.connection.error.set(new Error('connection failed'))
    await settle()
    let attempts = 0
    for (const delay of [2000, 4000, 8000, 16000, 30000, 30000]) {
        t.mock.timers.tick(delay - 1)
        await settle()
        assert.equal(f.refreshes(), attempts)
        t.mock.timers.tick(1)
        await settle()
        assert.equal(f.refreshes(), ++attempts)
    }
    f.signals.close()
    t.mock.timers.tick(60000)
    await settle()
    assert.equal(f.refreshes(), attempts)
})
