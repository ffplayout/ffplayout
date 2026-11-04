import { authFetch } from '@/composables/authFetch'

export type SelectionMode = 'file' | 'directory' | 'both'
export type SelectionPathFormat = 'relative' | 'absolute'
export interface BrowserEntry {
    name: string
    path: string
    size?: number
}
export interface BrowserListing {
    path: string
    /** The first breadcrumb identifies the root for relative selections. */
    breadcrumbs: BrowserEntry[]
    folders: BrowserEntry[]
    files: BrowserEntry[]
    selected_path: string | null
    fallback: boolean
}
export type BrowserLoader = (path: string, initial: boolean, signal: AbortSignal) => Promise<BrowserListing>

export function formatBrowserSelection(path: string, root: string, format: SelectionPathFormat): string {
    if (format === 'absolute') return path
    const normalize = (value: string) =>
        value
            .replaceAll('\\', '/')
            .replace(/\/+/g, '/')
            .split('/')
            .filter((part) => part !== '.')
            .join('/')
            .replace(/\/+$/, '')
    const normalizedPath = normalize(path)
    const normalizedRoot = normalize(root)
    if (normalizedPath === normalizedRoot) return '.'
    const prefix = `${normalizedRoot}/`
    if (!normalizedPath.startsWith(prefix)) throw new Error('Selection is outside the browser root')
    return normalizedPath.slice(prefix.length)
}

export function browseChannelPaths(channel: number, source: string, initial: boolean, signal: AbortSignal) {
    return authFetch<BrowserListing>(`/api/file/${channel}/browse-selection`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ source, initial }),
        signal,
    })
}
