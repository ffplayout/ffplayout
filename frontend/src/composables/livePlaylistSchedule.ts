import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

dayjs.extend(utc)
dayjs.extend(timezone)

/** Match the backend's broadcast day and wall-clock schedule, including after midnight. */
export function broadcastPosition(timestamp: number, zone: string, start: number, shift = 0) {
    const now = dayjs(timestamp).tz(zone)
    const seconds = now.hour() * 3600 + now.minute() * 60 + now.second() + now.millisecond() / 1000
    const beforeStart = seconds < start
    return {
        date: (beforeStart ? now.subtract(1, 'day') : now).format('YYYY-MM-DD'),
        elapsed: seconds + (beforeStart ? 86400 : 0) - start + shift,
    }
}

/** Read-only schedule projection; never changes the running playout or editor draft. */
export interface ScheduledClip {
    source: string
    title?: string | null
    in: number
    out: number
    duration: number
    ad?: boolean
}

export interface ScheduledPlaylistPosition {
    channel: number
    date: string
    index: number
    clip: ScheduledClip
}

/** Never highlight a different day or a row moved/replaced in an unsaved draft. */
export function isScheduledRow(
    position: ScheduledPlaylistPosition | undefined,
    channel: number | undefined,
    date: string,
    index: number,
    clip: ScheduledClip,
) {
    return Boolean(
        position &&
        position.channel === channel &&
        position.date === date &&
        position.index === index &&
        position.clip.source === clip.source &&
        position.clip.in === clip.in &&
        position.clip.out === clip.out,
    )
}

export function scheduledRowIndex(
    position: ScheduledPlaylistPosition | undefined,
    channel: number | undefined,
    date: string,
    program: readonly ScheduledClip[],
): number {
    const clip = position && program[position.index]
    return position && clip && isScheduledRow(position, channel, date, position.index, clip) ? position.index : -1
}

export function scheduledClipAt<T extends ScheduledClip>(
    program: readonly T[],
    secondsSinceStart: number,
    length: number,
    infinite = false,
) {
    if (!Number.isFinite(secondsSinceStart) || secondsSinceStart < 0) return undefined
    const total = program.reduce((sum, clip) => sum + Math.max(0, clip.out - clip.in), 0)
    if (total <= 0 || !Number.isFinite(total)) return undefined
    let elapsed = infinite ? secondsSinceStart % total : secondsSinceStart
    if (!infinite && elapsed >= length) return undefined
    let begin = 0
    for (const [index, clip] of program.entries()) {
        const duration = Math.max(0, clip.out - clip.in)
        if (elapsed < duration) {
            const remaining = Math.min(duration - elapsed, infinite ? Infinity : length - secondsSinceStart)
            return { clip, index, remaining, progress: duration > 0 ? (elapsed / duration) * 100 : 0 }
        }
        elapsed -= duration
        begin += duration
        if (!infinite && begin >= length) break
    }
    return undefined
}

/** Keep ordinary playback information when live input interrupts folder playback. */
export function playoutDisplay(
    mode: string,
    ingest: boolean,
    current: ScheduledClip,
    elapsed: number,
    progress: number,
    scheduled?: { clip: ScheduledClip; remaining: number; progress: number },
) {
    if (ingest && mode === 'playlist') {
        return { clip: scheduled?.clip, remaining: scheduled?.remaining, progress: scheduled?.progress ?? 0 }
    }
    return { clip: current, remaining: Math.max(0, current.out - elapsed), progress }
}
