/** Map a browser drag to even program pixels, including off-canvas placement. */
export function graphicDragPosition(
    origin: readonly [number, number],
    delta: readonly [number, number],
    canvas: readonly [number, number],
    preview: readonly [number, number],
): [number, number] {
    return [0, 1].map((axis) => {
        if (!Number.isFinite(preview[axis]) || preview[axis] <= 0) return origin[axis]
        const pixels = origin[axis] + (delta[axis] * canvas[axis]) / preview[axis]
        return Number.isFinite(pixels)
            ? Math.max(-2147483648, Math.min(2147483646, Math.round(pixels / 2) * 2))
            : origin[axis]
    }) as [number, number]
}

/** Project a corner drag onto the source aspect ratio in program pixels. */
export function graphicResizeSize(
    origin: readonly [number, number],
    delta: readonly [number, number],
    canvas: readonly [number, number],
    preview: readonly [number, number],
    source: readonly [number, number] = origin,
): [number, number] {
    if (
        ![...origin, ...canvas, ...preview, ...source].every((value) => Number.isFinite(value) && value > 0) ||
        !delta.every(Number.isFinite)
    )
        return [...origin]
    const width = origin[0] + (delta[0] * canvas[0]) / preview[0]
    const height = origin[1] + (delta[1] * canvas[1]) / preview[1]
    const projected = (width * source[0] + height * source[1]) / (source[0] ** 2 + source[1] ** 2)
    if (!Number.isFinite(projected)) return [...origin]
    const minimum = Math.max(2 / source[0], 2 / source[1])
    const maximum = Math.min(8192 / source[0], 8192 / source[1])
    const scale = Math.min(maximum, Math.max(minimum, projected))
    return source.map((value) => Math.max(2, Math.min(8192, Math.round((value * scale) / 2) * 2))) as [number, number]
}

/** Resize from either corner while keeping the opposite corner fixed. */
export function graphicResizeFromCorner(
    position: readonly [number, number],
    origin: readonly [number, number],
    delta: readonly [number, number],
    canvas: readonly [number, number],
    preview: readonly [number, number],
    source: readonly [number, number],
    fromStart = false,
): { position: [number, number]; size: [number, number] } {
    const direction: [number, number] = fromStart ? [-delta[0], -delta[1]] : [...delta]
    const size = graphicResizeSize(origin, direction, canvas, preview, source)
    return {
        position: fromStart ? [position[0] + origin[0] - size[0], position[1] + origin[1] - size[1]] : [...position],
        size,
    }
}

/** Fit oversized graphics into the program canvas while respecting YUVA size limits. */
export function graphicFitSize(source: readonly [number, number], canvas: readonly [number, number]): [number, number] {
    if (![...source, ...canvas].every((value) => Number.isFinite(value) && value > 0)) return [2, 2]
    const scale = Math.min(canvas[0] / source[0], canvas[1] / source[1], 8192 / source[0], 8192 / source[1])
    return source.map((value) => Math.max(2, Math.floor((value * scale) / 2) * 2)) as [number, number]
}

/** Snap graphic centers within six preview pixels, independently on each axis. */
export function graphicSnapPosition(
    position: readonly [number, number],
    size: readonly [number, number],
    canvas: readonly [number, number],
    preview: readonly [number, number],
): { position: [number, number]; guides: [boolean, boolean] } {
    const result: [number, number] = [...position]
    const guides: [boolean, boolean] = [false, false]
    for (const axis of [0, 1]) {
        if (
            ![size[axis], canvas[axis], preview[axis]].every((value) => Number.isFinite(value) && value > 0) ||
            !Number.isFinite(position[axis])
        )
            continue
        const center = (canvas[axis] - size[axis]) / 2
        if ((Math.abs(position[axis] - center) * preview[axis]) / canvas[axis] <= 6) {
            result[axis] = Math.round(center / 2) * 2
            guides[axis] = true
        }
    }
    return { position: result, guides }
}

/** Keep tiny selections draggable without changing the rendered graphic size. */
export function graphicSelectionRect(
    position: readonly [number, number],
    size: readonly [number, number],
    canvas: readonly [number, number],
    preview: readonly [number, number],
) {
    if (
        ![...canvas, ...preview, ...size].every((value) => Number.isFinite(value) && value > 0) ||
        !position.every(Number.isFinite)
    )
        return undefined
    const [width, height] = size.map((value, axis) => (value * preview[axis]) / canvas[axis])
    const compact = width < 48 || height < 48
    const dimensions = [width, height].map((value, axis) =>
        compact ? Math.min(preview[axis], Math.max(48, value)) : value,
    )
    const origin = position.map((value, axis) => {
        const actual = (value * preview[axis]) / canvas[axis]
        if (!compact) return actual
        const centered = actual + ([width, height][axis] - dimensions[axis]) / 2
        return Math.max(0, Math.min(preview[axis] - dimensions[axis], centered))
    })
    return { compact, left: origin[0], top: origin[1], width: dimensions[0], height: dimensions[1] }
}
