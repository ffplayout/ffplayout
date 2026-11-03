import assert from 'node:assert/strict'
import { test } from 'node:test'
import { graphicDragPosition } from '../graphicsLayout.ts'
import { formatLogoSize, parseLogoSize } from '../logoSize.ts'

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

test('logo sizing recognizes existing pixel, automatic and program-relative dimensions', () => {
    assert.deepEqual(parseLogoSize(''), { mode: 'original', width: '', height: '' })
    assert.deepEqual(parseLogoSize(' 320x180 '), { mode: 'pixels', width: 320, height: 180 })
    assert.deepEqual(parseLogoSize('100:-1'), { mode: 'pixels', width: 100, height: '' })
    assert.deepEqual(parseLogoSize('-1:12.5%'), { mode: 'percent', width: '', height: 12.5 })
})

test('logo sizing preserves legacy expressions and mixed units in the advanced editor', () => {
    for (const expression of ['iw:ih', 'W:-1', '100:12%', '12%:100', '100', 'Infinity%:-1']) {
        assert.equal(parseLogoSize(expression).mode, 'custom', expression)
    }
})

test('logo sizing round-trips both axes and automatic aspect ratio in existing storage format', () => {
    for (const value of ['100:-1', '-1:80', '320:180', '12.5%:-1', '-1:10%', '20%:10%']) {
        assert.equal(formatLogoSize(parseLogoSize(value)), value)
    }
    assert.equal(formatLogoSize({ mode: 'original', width: '', height: '' }), '')
    assert.equal(formatLogoSize({ mode: 'pixels', width: '', height: '' }), '-1:-1')
})
