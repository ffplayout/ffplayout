import assert from 'node:assert/strict'
import { test } from 'node:test'
import { randomId } from '../randomId.ts'

const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/

test('ids are UUIDs where randomUUID is available', () => {
    assert.match(randomId(), UUID_V4)
})

test('ids are still unique UUIDs without randomUUID, as on plain HTTP', (t) => {
    Object.defineProperty(crypto, 'randomUUID', { value: undefined, configurable: true })
    t.after(() => {
        delete (crypto as { randomUUID?: unknown }).randomUUID
    })

    const first = randomId()
    const second = randomId()

    assert.match(first, UUID_V4)
    assert.match(second, UUID_V4)
    assert.notEqual(first, second)
})
