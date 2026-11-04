import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
    broadcastPosition,
    playoutDisplay,
    isScheduledRow,
    scheduledClipAt,
    scheduledRowIndex,
} from '../livePlaylistSchedule.ts'

const program = [
    { source: 'first.mp4', in: 10, out: 20, duration: 60 },
    { source: 'empty.mp4', in: 0, out: 0, duration: 0 },
    { source: 'second.mp4', in: 5, out: 25, duration: 60 },
]

test('live countdown advances across trimmed and empty clips without changing the playlist', () => {
    const original = JSON.stringify(program)
    const first = scheduledClipAt(program, 9.5, 86400)!
    assert.equal(first.clip.source, 'first.mp4')
    assert.equal(first.remaining, 0.5)
    assert.equal(first.progress, 95)
    const second = scheduledClipAt(program, 10, 86400)!
    assert.equal(second.clip.source, 'second.mp4')
    assert.equal(second.index, 2)
    assert.equal(second.remaining, 20)
    assert.equal(second.progress, 0)
    assert.equal(scheduledClipAt(program, 29, 86400)?.remaining, 1)
    assert.equal(scheduledClipAt(program, 30, 86400), undefined)
    assert.equal(JSON.stringify(program), original)
})

test('schedule caps the last clip at the broadcast end and only loops in infinite mode', () => {
    assert.equal(scheduledClipAt(program, 24, 25)?.remaining, 1)
    assert.equal(scheduledClipAt(program, 25, 25), undefined)
    assert.equal(scheduledClipAt(program, 30, 86400, true)?.clip.source, 'first.mp4')
    assert.equal(scheduledClipAt(program, 40, 86400, true)?.remaining, 20)
    assert.equal(scheduledClipAt([], 2, 86400), undefined)
    assert.equal(scheduledClipAt(program, -1, 86400), undefined)
    assert.equal(scheduledClipAt(program, NaN, 86400), undefined)
})

test('broadcast date follows the configured start and server timezone, including midnight', () => {
    assert.deepEqual(broadcastPosition(Date.parse('2026-11-03T04:59:59Z'), 'Europe/Berlin', 6 * 3600), {
        date: '2026-11-02',
        elapsed: 86399,
    })
    assert.deepEqual(broadcastPosition(Date.parse('2026-11-03T05:00:00Z'), 'Europe/Berlin', 6 * 3600), {
        date: '2026-11-03',
        elapsed: 0,
    })
    assert.deepEqual(broadcastPosition(Date.parse('2026-11-03T23:00:05Z'), 'Europe/Berlin', 18 * 3600, 3), {
        date: '2026-11-03',
        elapsed: 21608,
    })
    assert.deepEqual(broadcastPosition(Date.parse('2026-11-03T00:00:00Z'), 'UTC', 0), {
        date: '2026-11-03',
        elapsed: 0,
    })
})

test('green marking follows the same schedule and ignores other channels, dates and edited rows', () => {
    const position = { ...scheduledClipAt(program, 10, 86400)!, channel: 1, date: '2026-11-03' }
    assert.equal(isScheduledRow(position, 1, position.date, 2, program[2]), true)
    assert.equal(isScheduledRow(position, 2, position.date, 2, program[2]), false)
    assert.equal(isScheduledRow(position, 1, '2026-11-02', 2, program[2]), false)
    assert.equal(isScheduledRow(position, 1, position.date, 0, program[2]), false)
    assert.equal(isScheduledRow(position, 1, position.date, 2, { ...program[2], in: 0 }), false)
    assert.equal(isScheduledRow(undefined, 1, position.date, 2, program[2]), false)
})

test('scroll target becomes available after either load order and accepts the first clip', () => {
    const position = { ...scheduledClipAt(program, 0, 86400)!, channel: 1, date: '2026-11-03' }
    assert.equal(scheduledRowIndex(undefined, 1, position.date, program), -1)
    assert.equal(scheduledRowIndex(position, 1, position.date, []), -1)
    assert.equal(scheduledRowIndex(position, 1, position.date, program), 0)
    assert.equal(scheduledRowIndex(position, 1, '2026-11-04', program), -1)
    const next = { ...position, ...scheduledClipAt(program, 10, 86400)! }
    assert.equal(scheduledRowIndex(next, 1, next.date, program), 2)
})

test('live folder playback preserves clip, remaining time and progress', () => {
    const display = playoutDisplay('folder', true, program[0], 12, 60)
    assert.equal(display.clip, program[0])
    assert.equal(display.remaining, 8)
    assert.equal(display.progress, 60)
    assert.equal(playoutDisplay('folder', true, program[0], 30, 100).remaining, 0)
})

test('playlist display distinguishes live projection, missing data and normal playback', () => {
    const position = scheduledClipAt(program, 10, 86400)!
    const display = playoutDisplay('playlist', true, program[0], 30, 100, position)
    assert.equal(display.clip, program[2])
    assert.equal(display.remaining, 20)
    assert.equal(playoutDisplay('playlist', true, program[0], 30, 100).clip, undefined)
    assert.equal(playoutDisplay('playlist', false, program[0], 12, 60, position).clip, program[0])
})
