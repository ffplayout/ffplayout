import { authFetch } from '@/composables/authFetch'

type FileAccessResponse = {
    access: string
    expires_in_seconds: number
}

function fileUrl(channelId: number | undefined, path: string, access: string): string {
    const encodedPath = encodeURIComponent(`/file/${channelId}/${path.replace(/^\/+/, '')}`).replace(/%2F/g, '/')

    return `${encodedPath}?access=${encodeURIComponent(access)}`
}

export async function createFilePreviewUrl(channelId: number | undefined, path: string): Promise<string> {
    if (!channelId) {
        throw new Error('Missing channel id')
    }

    const token = await authFetch<FileAccessResponse>(`/api/file/${channelId}/access-token`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify({ filename: path }),
    })

    return fileUrl(channelId, path, token.access)
}

/** Playlist paths are storage-relative; browser names also include its current folder. */
export function storagePreviewPath(path: string, root: string, folder?: string): string {
    if (/^[a-z][a-z0-9+.-]*:\/\//i.test(path)) return path
    const absolute = path.startsWith('/') || /^[a-z]:[\\/]/i.test(path)
    if (absolute) return path
    return [root.replace(/[\\/]+$/, ''), folder, path].filter(Boolean).join('/')
}
