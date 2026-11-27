const LOG_SPAN_OPEN = /&lt;span class=&quot;([a-z][a-z-]*)&quot;&gt;/g
const LOG_SPAN_CLOSE = /&lt;\/span&gt;/g

function escapeHtml(text: string): string {
    return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
}

/**
 * Make server log text safe for `v-html`.
 *
 * Log lines contain file names and other values that users control, so all
 * markup is escaped. Only the `<span class="...">` highlighting written by the
 * logger is restored.
 */
export function sanitizeLogHtml(log: string): string {
    return escapeHtml(log).replace(LOG_SPAN_OPEN, '<span class="$1">').replace(LOG_SPAN_CLOSE, '</span>')
}
