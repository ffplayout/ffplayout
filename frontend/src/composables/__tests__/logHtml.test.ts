import assert from 'node:assert/strict'
import { test } from 'node:test'
import { sanitizeLogHtml } from '../logHtml.ts'

test('logger highlighting is kept', () => {
    const line = '[<span class="level-info"> INFO</span>] Play <span class="log-addr">"a.mp4"</span>'

    assert.equal(sanitizeLogHtml(line), line.replace(/"a\.mp4"/, '&quot;a.mp4&quot;'))
})

test('markup from file names is escaped', () => {
    const line = 'Create new file: <span class="log-addr">"<img src=x onerror=alert(1)>.mp4"</span>'
    const html = sanitizeLogHtml(line)

    assert.ok(!html.includes('<img'))
    assert.ok(html.includes('&lt;img src=x onerror=alert(1)&gt;'))
})

test('spans with other attributes are not restored', () => {
    for (const line of [
        '<span class="log-addr" onmouseover="alert(1)">x</span>',
        '<span onclick="alert(1)">x</span>',
        '<span class="a b">x</span>',
        '<script>alert(1)</script>',
    ]) {
        assert.ok(!/<(?!\/span>)/.test(sanitizeLogHtml(line)), line)
    }
})
