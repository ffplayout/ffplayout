import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
    graphicDragPosition,
    graphicFitSize,
    graphicResizeSize,
    graphicResizeFromCorner,
    graphicSnapPosition,
    graphicSelectionRect,
} from '../graphicsLayout.ts'
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

test('corner resizing uses program pixels and source aspect ratio at different preview scales', () => {
    assert.deepEqual(graphicResizeSize([1280, 720], [80, 45], [1920, 1080], [480, 270], [3840, 2160]), [1600, 900])
    assert.deepEqual(graphicResizeSize([1280, 720], [80, 90], [1920, 1080], [480, 540], [3840, 2160]), [1600, 900])
    assert.deepEqual(graphicResizeSize([360, 640], [45, 80], [1920, 1080], [960, 540], [1080, 1920]), [450, 800])
    const [width, height] = graphicResizeSize([1000, 1000], [0, 0], [1280, 720], [640, 360], [16, 9])
    assert.ok(Math.abs(width * 9 - height * 16) <= 25, 'a stretched override must return to the source aspect ratio')
})

test('proportional resizing stays even and within the compositor bounds', () => {
    assert.deepEqual(graphicResizeSize([1280, 720], [1e9, 1e9], [1920, 1080], [480, 270], [16, 9]), [8192, 4608])
    assert.deepEqual(graphicResizeSize([1280, 720], [-1e9, -1e9], [1920, 1080], [480, 270], [16, 9]), [4, 2])
    assert.deepEqual(graphicResizeSize([1280, 720], [10, 10], [1920, 1080], [0, 0], [16, 9]), [1280, 720])
    assert.deepEqual(graphicResizeSize([1280, 720], [Infinity, NaN], [1920, 1080], [480, 270], [16, 9]), [1280, 720])
})

test('fitting oversized and portrait sources preserves aspect and makes corner handles reachable', () => {
    assert.deepEqual(graphicFitSize([3840, 2160], [1280, 720]), [1280, 720])
    assert.deepEqual(graphicFitSize([1080, 1920], [1280, 720]), [404, 720])
    assert.deepEqual(graphicFitSize([3840, 2160], [16384, 9216]), [8192, 4608])
    assert.deepEqual(graphicFitSize([0, 0], [1280, 720]), [2, 2])
})

test('center snapping uses preview distance and shows independent alignment guides', () => {
    assert.deepEqual(graphicSnapPosition([790, 438], [320, 180], [1920, 1080], [480, 270]), {
        position: [800, 450],
        guides: [true, true],
    })
    assert.deepEqual(graphicSnapPosition([774, 450], [320, 180], [1920, 1080], [480, 270]), {
        position: [774, 450],
        guides: [false, true],
    })
    assert.deepEqual(graphicSnapPosition([790, 430], [320, 180], [1920, 1080], [1920, 1080]), {
        position: [790, 430],
        guides: [false, false],
    })
    assert.deepEqual(graphicSnapPosition([-100, 0], [2120, 1080], [1920, 1080], [480, 270]), {
        position: [-100, 0],
        guides: [true, true],
    })
    assert.deepEqual(graphicSnapPosition([10, 20], [0, NaN], [1920, 1080], [0, 270]), {
        position: [10, 20],
        guides: [false, false],
    })
})

test('top-left resizing anchors the bottom-right corner through growth, shrinkage and size limits', () => {
    for (const delta of [
        [-80, -45],
        [80, 45],
        [-1e9, -1e9],
        [1e9, 1e9],
    ] as const) {
        const result = graphicResizeFromCorner([100, 60], [1280, 720], delta, [1920, 1080], [480, 270], [16, 9], true)
        assert.equal(result.position[0] + result.size[0], 1380)
        assert.equal(result.position[1] + result.size[1], 780)
        assert.ok(result.size.every((value) => value >= 2 && value <= 8192 && value % 2 === 0))
    }
    assert.deepEqual(
        graphicResizeFromCorner([100, 60], [1280, 720], [-80, -45], [1920, 1080], [480, 270], [16, 9], true),
        {
            position: [-220, -120],
            size: [1600, 900],
        },
    )
    assert.deepEqual(graphicResizeFromCorner([100, 60], [1280, 720], [80, 45], [1920, 1080], [480, 270], [16, 9]), {
        position: [100, 60],
        size: [1600, 900],
    })
})

test('tiny selections expand only the interaction frame and keep a reachable drag area at canvas edges', () => {
    assert.deepEqual(graphicSelectionRect([100, 60], [16, 8], [1920, 1080], [480, 270]), {
        compact: true,
        left: 3,
        top: 0,
        width: 48,
        height: 48,
    })
    assert.deepEqual(graphicSelectionRect([1918, 1078], [2, 2], [1920, 1080], [480, 270]), {
        compact: true,
        left: 432,
        top: 222,
        width: 48,
        height: 48,
    })
    assert.deepEqual(graphicSelectionRect([0, 0], [2, 2], [1920, 1080], [20, 10]), {
        compact: true,
        left: 0,
        top: 0,
        width: 20,
        height: 10,
    })
    assert.equal(graphicSelectionRect([0, 0], [16, 8], [1920, 1080], [0, 0]), undefined)
})

test('selection handles depend on displayed size and retain exact geometry for larger graphics', () => {
    assert.deepEqual(graphicSelectionRect([100, 60], [192, 192], [1920, 1080], [480, 270]), {
        compact: false,
        left: 25,
        top: 15,
        width: 48,
        height: 48,
    })
    assert.equal(graphicSelectionRect([0, 0], [192, 192], [1920, 1080], [240, 135])?.compact, true)
    assert.equal(graphicSelectionRect([0, 0], [1000, 16], [1920, 1080], [480, 270])?.compact, true)
})
