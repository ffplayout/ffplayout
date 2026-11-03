import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ref, watch } from 'vue'
import { usePlaylistGraphicsValidation } from '../playlistGraphicsValidation.ts'
import { newGraphicEvent, readGraphicEvents, validateGraphicEvents, type GraphicEvent } from '../playlistGraphics.ts'

const take = (at = 0, fields: Partial<GraphicEvent> = {}): GraphicEvent => ({
    ...newGraphicEvent('take', at, 1),
    source: 'graphics/lower-third.png',
    ...fields,
})
const codes = (events: GraphicEvent[]) => validateGraphicEvents(events).map((issue) => issue.code)

test('optional graphics and unknown fields survive the form adapter without mutation', () => {
    assert.deepEqual(readGraphicEvents(undefined), [])
    const data = [take(0, { future_setting: { enabled: true } })]
    const original = JSON.stringify(data)
    assert.equal(readGraphicEvents(data), data)
    assert.deepEqual(codes(data), [])
    assert.equal(JSON.stringify(data), original)
    for (const unsupported of [
        { future_schema: 1 },
        [{ at: 0, action: 'future_action' }],
        [null],
        [take(0, { position: [0, 'center'] } as unknown as Partial<GraphicEvent>)],
        [take(0, { fade_in: null } as any)],
    ]) {
        const snapshot = JSON.stringify(unsupported)
        assert.equal(readGraphicEvents(unsupported), undefined)
        assert.equal(JSON.stringify(unsupported), snapshot)
    }
})

test('fade cycles include only the gaps between presentations and free the layer at the exact end', () => {
    // Two cycles of .5+4+.5 seconds with one ten-second gap finish at 20 seconds.
    const first = take(0, { fade_in: 0.5, hold: 4, fade_out: 0.5, pause: 10, repeat: 2 })
    assert.deepEqual(codes([first, take(20)]), [])
    assert.ok(codes([first, take(19.999)]).includes('activeLayer'))
    // Source completion does not retire an indefinite layer presentation.
    assert.ok(codes([take(0, { source_repeat: 1, source_end: 'hide' }), take(20)]).includes('activeLayer'))
})

test('Stop in a repeat pause cancels future cycles; an exit cannot be extended by a second Stop', () => {
    const repeating = take(0, { hold: 1, fade_out: 1, pause: 10, repeat: 'forever' })
    assert.deepEqual(codes([repeating, { at: 3, action: 'stop', layer: 1 }, take(3)]), [])
    assert.deepEqual(
        codes([
            take(0, { fade_in: 1, fade_out: 2 }),
            { at: 0.5, action: 'stop', layer: 1 },
            { at: 1, action: 'stop', layer: 1 },
            take(2.5),
        ]),
        [],
    )
})

test('equal-time Clear/Take preserves declaration order and other inactive-layer operations are rejected', () => {
    assert.deepEqual(codes([take(), { at: 1, action: 'clear', layer: 1 }, take(1)]), [])
    assert.ok(codes([take(), take(1), { at: 1, action: 'clear', layer: 1 }]).includes('activeLayer'))
    assert.deepEqual(codes([take(), { at: 1, action: 'clear_all' }, take(1)]), [])
    for (const action of ['update', 'visibility', 'stop', 'clear'] as const)
        assert.ok(codes([{ at: 0, action, layer: 9, visible: false }]).includes('unknownLayer'))
})

test('repeat validation, malformed numbers and microsecond ordering match the playlist contract', () => {
    for (const repeat of ['forever', 2] as const) {
        assert.ok(codes([take(0, { repeat })]).includes('repeatHold'))
        assert.ok(codes([take(0, { repeat, hold: 0 })]).includes('repeatHold'))
        assert.deepEqual(codes([take(0, { repeat, hold: 0, fade_in: 0.2 })]), [])
    }
    for (const count of [0, -1, 1.5, NaN, Infinity])
        assert.ok(codes([take(0, { source_repeat: count })]).includes('repeat'))
    assert.ok(codes([take(NaN)]).includes('time'))
    assert.ok(codes([take(0, { opacity: NaN })]).includes('opacity'))
    assert.ok(codes([take(0, { size: [101, 100] })]).includes('size'))
    assert.ok(codes([take(0, { position: [0x8000_0000, 0] })]).includes('position'))
    assert.ok(codes([take(2), { at: 1, action: 'clear_all' }]).includes('order'))
    assert.deepEqual(codes([take(0.00000049), { at: 0, action: 'clear_all' }]), [])
})

test('bounded event and active-layer counts are enforced', () => {
    assert.ok(codes(Array.from({ length: 257 }, (_, at) => ({ at, action: 'clear_all' }))).includes('eventLimit'))
    assert.ok(codes(Array.from({ length: 33 }, (_, layer) => take(0, { layer }))).includes('layerLimit'))
})

test('shortened clips ignore later layer operations instead of corrupting their timeline', () => {
    const events: GraphicEvent[] = [take(), { at: 5, action: 'clear', layer: 999 }, take(6)]
    assert.deepEqual(validateGraphicEvents(events, 5), [])
    assert.ok(validateGraphicEvents(events, 10).some((issue) => issue.code === 'unknownLayer'))
    assert.equal(events.length, 3, 'trimming must not delete future actions from stored JSON')
})

test('extending or moving In exposes conflicting takes and updates the save gate without editing graphics', (t) => {
    const original = [take(0), take(6)]
    const value = ref<unknown>(original)
    const input = ref(0)
    const output = ref(5)
    const state = usePlaylistGraphicsValidation(
        () => value.value,
        () => output.value - input.value,
    )
    const reported: boolean[] = []
    const stop = watch(state.canSave, (valid) => reported.push(valid), { flush: 'sync', immediate: true })
    t.after(stop)
    assert.equal(state.canSave.value, true)
    output.value = 10
    assert.equal(state.modified.value, false)
    assert.equal(state.canSave.value, false)
    assert.ok(state.issues.value.some((issue) => issue.code === 'activeLayer'))
    input.value = 5
    assert.equal(state.canSave.value, true)
    input.value = 0
    assert.equal(state.canSave.value, false)
    output.value = 5
    assert.equal(state.canSave.value, true)
    assert.deepEqual(reported, [true, false, true, false, true])
    assert.equal(JSON.stringify(value.value), JSON.stringify(original), 'timing edits must not rewrite actions')
})

test('initially invalid or unsupported JSON can still be preserved until its graphics are edited', () => {
    for (const original of [{ future_schema: true }, [take(0), take(1)]]) {
        const value = ref<unknown>(original)
        const duration = ref(10)
        const state = usePlaylistGraphicsValidation(
            () => value.value,
            () => duration.value,
        )
        assert.equal(state.canSave.value, true)
        duration.value = 20
        assert.equal(state.canSave.value, true)
        assert.equal(JSON.stringify(value.value), JSON.stringify(original))
        state.modified.value = true
        assert.equal(state.canSave.value, false)
        value.value = []
        assert.equal(state.canSave.value, true)
    }
})

test('ignored take semantics do not block edits to a visible graphic, but activating the future take does', () => {
    const value = ref<unknown>([
        take(0),
        take(20, {
            layer: 2,
            repeat: 'forever',
            hold: null,
            source: 'https://example.invalid/overlay.png',
            opacity: 2,
            size: [101, 100],
            fade_in: -1,
        }),
    ])
    const original = JSON.stringify(value.value)
    const duration = ref(10)
    const state = usePlaylistGraphicsValidation(
        () => value.value,
        () => duration.value,
    )
    assert.deepEqual(state.issues.value, [])
    state.modified.value = true
    value.value = (value.value as GraphicEvent[]).map((event, index) =>
        index === 0 ? { ...event, opacity: 0.8 } : event,
    )
    assert.equal(state.canSave.value, true)
    assert.equal(JSON.stringify((value.value as GraphicEvent[])[1]), JSON.stringify(JSON.parse(original)[1]))
    duration.value = 21
    assert.equal(state.canSave.value, false)
    assert.ok(state.issues.value.some((issue) => issue.index === 1))
})

test('ignored events still require valid JSON field types, times and declaration ordering', () => {
    assert.ok(validateGraphicEvents([take(20, { repeat: 0 })], 10).some((issue) => issue.code === 'repeat'))
    assert.ok(
        validateGraphicEvents([take(20, { position: [0x8000_0000, 0] })], 10).some(
            (issue) => issue.code === 'position',
        ),
    )
    assert.ok(validateGraphicEvents([take(20, { fade_in: NaN })], 10).some((issue) => issue.code === 'time'))
    assert.ok(validateGraphicEvents([take(NaN)], 10).some((issue) => issue.code === 'time'))
    assert.ok(validateGraphicEvents([take(20), take(19)], 10).some((issue) => issue.code === 'order'))
    assert.ok(
        validateGraphicEvents([{ at: 20, action: 'visibility', layer: 1 }], 10).some(
            (issue) => issue.code === 'visible',
        ),
    )
})
