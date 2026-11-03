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
