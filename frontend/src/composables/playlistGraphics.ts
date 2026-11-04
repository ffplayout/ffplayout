/** Playlist JSON adapter shared by graphics forms; the backend validates files/decoding. */
export const graphicActions = ['take', 'update', 'stop', 'visibility', 'clear', 'clear_all'] as const
export type GraphicAction = (typeof graphicActions)[number]
export type GraphicRepeat = number | 'forever'
export interface GraphicEvent {
    [key: string]: unknown
    at: number
    action: GraphicAction
    layer?: number
    source?: string
    size?: [number, number] | null
    position?: [number, number] | null
    opacity?: number | null
    z_index?: number | null
    fade_in?: number
    hold?: number | null
    duration?: number | null
    fade_out?: number
    pause?: number
    repeat?: GraphicRepeat
    source_repeat?: GraphicRepeat
    source_end?: 'hide' | 'hold_last'
    visible?: boolean
}

/** Unknown fields survive editing; unsupported shapes remain untouched in the parent. */
export function readGraphicEvents(value: unknown): GraphicEvent[] | undefined {
    if (value == null) return []
    if (!Array.isArray(value)) return undefined
    const numeric = ['layer', 'opacity', 'z_index', 'fade_in', 'hold', 'duration', 'fade_out', 'pause']
    for (const item of value) {
        if (
            !item ||
            typeof item !== 'object' ||
            Array.isArray(item) ||
            typeof item.at !== 'number' ||
            !graphicActions.includes(item.action)
        )
            return undefined
        if (numeric.some((key) => item[key] != null && typeof item[key] !== 'number')) return undefined
        if (
            ['size', 'position'].some(
                (key) =>
                    item[key] != null &&
                    (!Array.isArray(item[key]) ||
                        item[key].length !== 2 ||
                        item[key].some((part: unknown) => typeof part !== 'number')),
            )
        )
            return undefined
        if (
            item.action === 'take' &&
            ['position', 'opacity', 'z_index', 'fade_in', 'fade_out', 'pause'].some((key) => item[key] === null)
        )
            return undefined
        if (item.source != null && typeof item.source !== 'string') return undefined
        if (item.visible != null && typeof item.visible !== 'boolean') return undefined
        if (
            ['repeat', 'source_repeat'].some(
                (key) => item[key] !== undefined && typeof item[key] !== 'number' && item[key] !== 'forever',
            )
        )
            return undefined
        if (item.source_end !== undefined && !['hide', 'hold_last'].includes(item.source_end)) return undefined
    }
    return value as GraphicEvent[]
}

export function newGraphicEvent(action: GraphicAction, at = 0, layer = 1): GraphicEvent {
    if (action === 'clear_all') return { at, action }
    if (action === 'take')
        return {
            at,
            action,
            layer,
            source: '',
            position: [0, 0],
            opacity: 1,
            z_index: 0,
            fade_in: 0,
            fade_out: 0,
            repeat: 1,
            source_repeat: 'forever',
            source_end: 'hide',
        }
    if (action === 'visibility') return { at, action, layer, visible: true }
    return { at, action, layer }
}

export interface GraphicIssue {
    index: number
    code: string
}
const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value)
const integer = (value: unknown, min: number, max: number) =>
    finite(value) && Number.isInteger(value) && value >= min && value <= max
const time = (value: unknown) => finite(value) && value >= 0 && value < Number.MAX_SAFE_INTEGER / 1_000_000
const repeat = (value: unknown) => value === 'forever' || integer(value, 1, 0xffff_ffff)
const micros = (value: number) => Math.round(value * 1_000_000)

interface ActiveLayer {
    start: number
    end: number
    entrance: number
    exit: number
    active?: number
    cycle?: number
    stopped: boolean
}

export function validateGraphicEvents(events: GraphicEvent[], clipDuration = Infinity): GraphicIssue[] {
    const issues: GraphicIssue[] = []
    const active = new Map<number, ActiveLayer>()
    let previous = 0
    const clipEnd = micros(clipDuration)
    const add = (index: number, code: string) => issues.push({ index, code })
    if (events.length > 256) add(-1, 'eventLimit')
    events.forEach((event, index) => {
        if (!time(event.at)) {
            add(index, 'time')
            return
        }
        const at = micros(event.at)
        if (at < previous) add(index, 'order')
        previous = at
        for (const [id, layer] of active) if (layer.end <= at) active.delete(id)
        if (event.action === 'clear_all') {
            if (at < clipEnd) active.clear()
            return
        }
        if (!integer(event.layer, 0, Number.MAX_SAFE_INTEGER)) {
            add(index, 'layer')
            return
        }
        const structuralStart = issues.length
        if (event.position != null && !event.position.every((v) => integer(v, -0x8000_0000, 0x7fff_ffff)))
            add(index, 'position')
        if (event.opacity != null && !finite(event.opacity)) add(index, 'opacity')
        if (event.z_index != null && !integer(event.z_index, -0x8000_0000, 0x7fff_ffff)) add(index, 'position')
        // Serde parses every event before the backend skips times past the clip
        // end. Keep wire-type/range checks here, but defer playback semantics.
        if (event.action === 'visibility' && typeof event.visible !== 'boolean') add(index, 'visible')
        if (event.action === 'take') {
            if (typeof event.source !== 'string') add(index, 'source')
            if (event.size != null && !event.size.every((v) => integer(v, 0, 0xffff_ffff))) add(index, 'size')
            if (
                ![
                    event.fade_in ?? 0,
                    event.fade_out ?? 0,
                    event.pause ?? 0,
                    event.hold ?? 0,
                    event.duration ?? 0,
                ].every(finite)
            )
                add(index, 'time')
            if (!repeat(event.repeat ?? 1) || !repeat(event.source_repeat ?? 'forever')) add(index, 'repeat')
        }
        if (issues.length > structuralStart || at >= clipEnd) return
        if (event.opacity != null && (event.opacity < 0 || event.opacity > 1)) add(index, 'opacity')
        if (event.action === 'take') {
            if (!event.source!.trim() || event.source!.includes('://')) add(index, 'source')
            if (event.size != null && !event.size.every((v) => integer(v, 2, 8192) && v % 2 === 0)) add(index, 'size')
            if (
                ![event.fade_in ?? 0, event.fade_out ?? 0, event.pause ?? 0].every(time) ||
                (event.hold != null && !time(event.hold)) ||
                (event.duration != null && !time(event.duration))
            ) {
                add(index, 'time')
                return
            }
            if (event.duration != null && event.hold != null) add(index, 'duration')
            let entrance = micros(event.fade_in ?? 0)
            let exit = micros(event.fade_out ?? 0)
            const requestedDuration =
                event.duration != null
                    ? micros(event.duration)
                    : event.hold != null
                      ? entrance + micros(event.hold) + exit
                      : Number.isFinite(clipEnd)
                        ? clipEnd - at
                        : undefined
            const finiteDuration =
                requestedDuration === undefined ? undefined : Math.min(requestedDuration, clipEnd - at)
            const duration = finiteDuration ?? 0
            if (finiteDuration !== undefined && entrance + exit > duration) {
                entrance = Math.floor((duration * entrance) / (entrance + exit))
                exit = duration - entrance
            }
            const cycle = duration + micros(event.pause ?? 0)
            const repeats = event.repeat ?? 1
            if (repeats !== 1 && ((event.duration == null && event.hold == null) || duration === 0))
                add(index, 'repeatHold')
            if (active.has(event.layer!)) add(index, 'activeLayer')
            const end =
                finiteDuration === undefined || repeats === 'forever' ? Infinity : at + duration + cycle * (repeats - 1)
            active.set(event.layer!, {
                start: at,
                end,
                entrance,
                exit,
                active: finiteDuration,
                cycle: finiteDuration === undefined ? undefined : cycle,
                stopped: false,
            })
            if (active.size > 32) add(index, 'layerLimit')
            return
        }
        const layer = active.get(event.layer!)
        if (!layer) {
            add(index, 'unknownLayer')
            return
        }
        if (event.action === 'clear') active.delete(event.layer!)
        if (event.action === 'stop' && !layer.stopped) {
            const elapsed = at - layer.start
            const phase = layer.cycle ? elapsed % layer.cycle : elapsed
            const transparent =
                (layer.entrance > 0 && phase === 0) || (layer.active !== undefined && phase >= layer.active)
            layer.end =
                at +
                (transparent
                    ? 0
                    : Math.min(
                          layer.exit,
                          layer.end - at,
                          layer.active === undefined ? Infinity : layer.active - phase,
                      ))
            layer.stopped = true
        }
    })
    return issues
}
