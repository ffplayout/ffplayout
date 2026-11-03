import assert from 'node:assert/strict'
import { test } from 'node:test'
import { graphicDragPosition } from '../graphicsLayout.ts'

test('graphic dragging uses program pixels instead of browser size and preserves clipping', () => {
    assert.deepEqual(graphicDragPosition([100, 50], [10, 10], [1920, 1080], [480, 270]), [140, 90])
    assert.deepEqual(graphicDragPosition([0, 0], [-5, -5], [1920, 1080], [480, 270]), [-20, -20])
    assert.deepEqual(graphicDragPosition([0, 0], [10.3, 0.4], [1920, 1080], [480, 270]), [42, 2])
})

test('collapsed previews and extreme drags cannot create invalid coordinates', () => {
    assert.deepEqual(graphicDragPosition([10, 20], [30, 40], [1920, 1080], [0, 0]), [10, 20])
    assert.deepEqual(graphicDragPosition([0, 0], [Infinity, NaN], [1920, 1080], [480, 270]), [0, 0])
    assert.deepEqual(graphicDragPosition([0, 0], [1e20, -1e20], [1920, 1080], [480, 270]), [2147483646, -2147483648])
})
