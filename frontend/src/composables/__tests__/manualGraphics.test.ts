import assert from 'node:assert/strict'
import { test } from 'node:test'
import { useManualGraphicsControl, type ManualGraphicsStatus } from '../manualGraphics.ts'

function status(revision: number): ManualGraphicsStatus {
    return {
        revision,
        running: true,
        canvas: [1280, 720],
        layers: [
            {
                layer: 1,
                name: 'Saved',
                settings: { action: 'take', at: 0, layer: 1, source: 'logo.png' },
                running: true,
                visible: true,
                active: true,
            },
        ],
    }
}

function deferred<T>() {
    let resolve!: (value: T) => void
    const promise = new Promise<T>((complete) => {
        resolve = complete
    })
    return { promise, resolve }
}

test('a failed status poll retains the current layer instances until a successful refresh', async () => {
    let fail = false
    let revision = 1
    const control = useManualGraphicsControl(
        () => 1,
        async <T>() => {
            if (fail) throw new Error('Connection lost')
            return status(revision) as T
        },
    )
    await control.refresh()
    const last = control.status.value
    const layer = last!.layers[0]
    fail = true
    await control.refresh()
    assert.equal(control.status.value, last)
    assert.equal(control.status.value!.layers[0], layer)
    assert.equal(control.error.value, 'Connection lost')
    fail = false
    revision = 2
    await control.refresh()
    assert.equal(control.status.value!.revision, 2)
    assert.equal(control.error.value, '')
    control.reset()
    assert.equal(control.status.value, undefined)
})

test('a save finishing its status refresh after a channel change cannot succeed for the new editor', async () => {
    const refreshing = deferred<void>()
    const oldStatus = deferred<ManualGraphicsStatus>()
    let channel = 1
    let firstReads = 0
    const control = useManualGraphicsControl(
        () => channel,
        async <T>(url: string, options?: RequestInit) => {
            if (options?.method === 'POST') return { revision: 2 } as T
            if (url === '/api/graphics/1' && firstReads++ === 1) {
                refreshing.resolve()
                return (await oldStatus.promise) as T
            }
            return status(channel === 1 ? 1 : 20) as T
        },
    )
    await control.refresh()
    const saved = control.command({ action: 'save', layer: 1 })
    await refreshing.promise
    channel = 2
    control.reset()
    await control.refresh()
    oldStatus.resolve(status(2))
    assert.equal(await saved, false)
    assert.equal(control.status.value!.revision, 20)
    assert.equal(control.busy.value, false)
    control.reset()
})

test('leaving and returning to the same channel still invalidates an old command', async () => {
    const sent = deferred<void>()
    const response = deferred<object>()
    const control = useManualGraphicsControl(
        () => 1,
        async <T>(_url: string, options?: RequestInit) => {
            if (options?.method === 'POST') {
                sent.resolve()
                return (await response.promise) as T
            }
            return status(1) as T
        },
    )
    await control.refresh()
    const pending = control.command({ action: 'save', layer: 1 })
    await sent.promise
    control.reset()
    await control.refresh()
    response.resolve({ revision: 2 })
    assert.equal(await pending, false)
    assert.equal(control.status.value!.revision, 1)
    control.reset()
})

test('successful polls clear only retrieval errors and retain rejected command errors', async () => {
    let failCommand = true
    let failPoll = false
    const control = useManualGraphicsControl(
        () => 1,
        async <T>(_url: string, options?: RequestInit) => {
            if (options?.method === 'POST') {
                if (failCommand) throw new Error('Invalid source')
                return { revision: 2 } as T
            }
            if (failPoll) throw new Error('Connection lost')
            return status(1) as T
        },
    )
    await control.refresh()
    assert.equal(await control.command({ action: 'take', layer: 1 }), false)
    assert.equal(control.error.value, 'Invalid source')
    failPoll = true
    await control.refresh()
    assert.equal(control.error.value, 'Invalid source')
    failPoll = false
    await control.refresh()
    assert.equal(control.error.value, 'Invalid source')
    failCommand = false
    assert.equal(await control.command({ action: 'take', layer: 1 }), true)
    assert.equal(control.error.value, '')
    control.reset()
})
