import assert from 'node:assert/strict'
import { test } from 'node:test'
import { Broadcast } from '@moq/net'
import { Effect, Signal } from '@moq/signals'
import { AudioLevelTimeline, synchronizeMoqAudioLevel } from '../src/composables/moqAudioLevelSync.ts'

const settle = async () => { for (let i = 0; i < 40; i++) await Promise.resolve() }
const reading = (ms, peak = -6) => ({ rms_db: -24, peak_db: peak, timestamp_us: ms * 1000 })

function fixture(t) {
    t.mock.timers.enable({ apis: ['setInterval', 'Date'] })
    const producer = new Broadcast.Producer()
    const track = producer.createTrack('audio-level.json', { maxAge: 0 })
    const active = new Signal(producer.consume())
    const signals = new Effect()
    const paused = new Signal(false)
    const muted = new Signal(false)
    const rendered = new Signal(undefined)
    const timestamp = new Signal(undefined)
    const stalled = new Signal(false)
    const context = { state: 'running', currentTime: 1, getOutputTimestamp: () => ({ contextTime: 0.9 }) }
    const readings = []
    synchronizeMoqAudioLevel({ signals, broadcast: { out: { active } }, controls: { paused, muted },
        renderer: { out: { timestamp: rendered } },
        audio: { out: { timestamp, stalled, context: new Signal(context) } } }, (level) => readings.push(level))
    t.after(() => { signals.close(); active.peek()?.close(); producer.close() })
    const publish = async (value) => {
        const group = track.appendGroup(); group.writeJson(value); group.close(); await settle()
    }
    return { publish, paused, muted, rendered, timestamp, stalled, context, readings, signals, track, active }
}

test('aligns measurement midpoints to audible playback, including output latency', async (t) => {
    const f = fixture(t)
    await settle()
    await f.publish(reading(10_000))
    f.timestamp.set(9_950)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1), null) // audible 9850 precedes the window midpoint 9875
    f.timestamp.set(10_000)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -6)
    await f.publish(reading(10_250, -2))
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -6)
    f.timestamp.set(10_400)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -2)
})

test('pause and stalls show current program readings, while disconnect clears the meter', async (t) => {
    const f = fixture(t)
    await settle()
    await f.publish(reading(10_000))
    f.timestamp.set(10_150)
    t.mock.timers.tick(50)
    f.paused.set(true)
    await f.publish(reading(10_250, 0))
    f.timestamp.set(10_400)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, 0)
    f.paused.set(false)
    f.stalled.set(true)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, 0)
    await f.publish(reading(10_500, -3))
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -3)
    f.stalled.set(false)
    f.timestamp.set(10_600)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -3)
    f.active.set(undefined)
    await settle()
    assert.equal(f.readings.at(-1), null)
    assert.equal(f.track.used.peek(), false)
})

test('untimed measurements never fall back to an unsynchronized display', async (t) => {
    const f = fixture(t)
    await settle()
    f.timestamp.set(10_150)
    await f.publish({ peak_db: -6, rms_db: -24 })
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1), null)
})

test('a long pause has a bounded history and backward seeks invalidate old readings', () => {
    const timeline = new AudioLevelTimeline()
    for (let i = 0; i < 1000; i++) timeline.push(reading(i * 250))
    assert.equal(timeline.at(100), null) // old history was evicted
    assert.equal(timeline.at(999 * 250).peak_db, -6)
    assert.equal(timeline.at(999 * 250 - 1000), null) // seek backwards
    timeline.push(reading(1000, -3))
    assert.equal(timeline.at(1000).peak_db, -3)
    assert.equal(timeline.at(2100), null) // no indefinitely held peak
})


test('muting follows the displayed video without requiring a running audio decoder', async (t) => {
    const f = fixture(t)
    await settle()
    f.muted.set(true)
    f.context.state = 'suspended'
    await f.publish(reading(10_000))
    f.rendered.set(9_800)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1), null)
    f.rendered.set(10_050)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -6)
})

test('a frozen video playhead switches to fresh program readings and expires stale data', async (t) => {
    const f = fixture(t)
    await settle()
    f.muted.set(true)
    f.rendered.set(10_000)
    await f.publish(reading(10_000))
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -6)
    await f.publish(reading(10_750, -2))
    t.mock.timers.tick(500)
    assert.equal(f.readings.at(-1).peak_db, -2)
    await f.publish(reading(11_000, -4))
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -4)
    t.mock.timers.tick(1100)
    assert.equal(f.readings.at(-1), null)
    f.rendered.set(10_900)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -4) // buffered reading still matches playback
})

test('network staleness preserves future measurements for audio still buffered in the player', async (t) => {
    const f = fixture(t)
    await settle()
    await f.publish(reading(10_000))
    f.timestamp.set(9_000)
    t.mock.timers.tick(2000)
    f.timestamp.set(10_150)
    t.mock.timers.tick(50)
    assert.equal(f.readings.at(-1).peak_db, -6)
})
