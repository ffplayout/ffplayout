import assert from 'node:assert/strict'
import { test } from 'node:test'
import { Broadcast } from '@moq/net'
import { Effect, Signal } from '@moq/signals'
import { subscribeMoqAudioLevel, subscribeMoqLoudness } from '../src/composables/moqAudioLevel.ts'

const settle = async () => {
    for (let i = 0; i < 30; i++) await Promise.resolve()
}

function fixture(t) {
    const producer = new Broadcast.Producer()
    const track = producer.createTrack('audio-level.json', { maxAge: 0 })
    const active = new Signal(producer.consume())
    const signals = new Effect()
    const readings = []
    subscribeMoqAudioLevel({ signals, broadcast: { out: { active } } }, (level) => readings.push(level))
    t.after(() => {
        signals.close()
        active.peek()?.close()
        producer.close()
    })
    return { producer, track, active, signals, readings }
}

function publish(track, value) {
    const group = track.appendGroup()
    group.writeJson(value)
    group.close()
}

test('receives program levels over a MoQ track and clears on broadcast disconnect', async (t) => {
    const f = fixture(t)
    await settle()
    publish(f.track, { peak_db: -6, rms_db: -24 })
    await settle()
    assert.deepEqual(f.readings.at(-1), { peak_db: -6, rms_db: -24 })
    publish(f.track, null)
    await settle()
    assert.equal(f.readings.at(-1), null)
    publish(f.track, { peak_db: -3, rms_db: -12 })
    await settle()
    assert.equal(f.readings.at(-1).peak_db, -3)
    f.active.set(undefined)
    await settle()
    assert.equal(f.readings.at(-1), null)
    assert.equal(f.track.used.peek(), false)
})

test('rejects malformed readings and releases the subscription on unmount', async (t) => {
    const f = fixture(t)
    await settle()
    publish(f.track, { peak_db: 'invalid', rms_db: -24 })
    await settle()
    assert.equal(f.readings.at(-1), null)
    publish(f.track, { peak_db: -4, rms_db: -20 })
    await settle()
    assert.equal(f.readings.at(-1).peak_db, -4)
    f.signals.close()
    await settle()
    assert.equal(f.track.used.peek(), false)
    const count = f.readings.length
    publish(f.track, { peak_db: 0, rms_db: 0 })
    await settle()
    assert.equal(f.readings.length, count)
})

test('resubscribes when reconnecting replaces the broadcast', async (t) => {
    const f = fixture(t)
    await settle()
    const replacement = new Broadcast.Producer()
    const track = replacement.createTrack('audio-level.json', { maxAge: 0 })
    const consumer = replacement.consume()
    t.after(() => { consumer.close(); replacement.close() })
    f.active.set(consumer)
    await settle()
    assert.equal(f.track.used.peek(), false)
    publish(track, { peak_db: -2, rms_db: -18 })
    await settle()
    assert.deepEqual(f.readings.at(-1), { peak_db: -2, rms_db: -18 })
})

test('clears a stale meter after one second without readings', async (t) => {
    t.mock.timers.enable({ apis: ['setInterval'] })
    let now = 0
    t.mock.method(performance, 'now', () => now)
    const f = fixture(t)
    await settle()
    publish(f.track, { peak_db: -6, rms_db: -24 })
    await settle()
    now = 750
    t.mock.timers.tick(750)
    assert.equal(f.readings.at(-1).peak_db, -6)
    now = 1250
    t.mock.timers.tick(500)
    assert.equal(f.readings.at(-1), null)
})


test('loudness uses a separate on-demand track and accepts unavailable metrics', async (t) => {
    const f = fixture(t)
    const track = f.producer.createTrack('audio-loudness.json', { maxAge: 0 })
    const readings = []
    subscribeMoqLoudness({ signals: f.signals, broadcast: { out: { active: f.active } } }, (value) => readings.push(value))
    await settle()
    const metrics = { momentary_lufs: -21, short_term_lufs: -23, integrated_lufs: -24, true_peak_dbtp: null }
    publish(track, metrics)
    await settle()
    assert.deepEqual(readings.at(-1), metrics)
    publish(track, { short_term_lufs: 'invalid' })
    await settle()
    assert.equal(readings.at(-1), null)
    f.signals.close()
    await settle()
    assert.equal(track.used.peek(), false)
})
