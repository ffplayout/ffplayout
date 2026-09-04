import { authFetch } from '@/composables/authFetch'

type FileAccessResponse = {
    access: string
    expires_in_seconds: number
}

function fileUrl(channelId: number | undefined, path: string, access: string): string {
    const encodedPath = encodeURIComponent(`/file/${channelId}${path}`).replace(/%2F/g, '/')

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
