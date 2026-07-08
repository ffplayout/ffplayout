type FileAccessResponse = {
    access: string
    expires_in_seconds: number
}

function fileUrl(channelId: number | undefined, path: string, access: string): string {
    const encodedPath = encodeURIComponent(`/file/${channelId}${path}`).replace(/%2F/g, '/')

    return `${encodedPath}?access=${encodeURIComponent(access)}`
}

export async function createFilePreviewUrl(
    channelId: number | undefined,
    path: string,
    authHeader: HeadersInit
): Promise<string> {
    if (!channelId) {
        throw new Error('Missing channel id')
    }

    const response = await fetch(`/api/file/${channelId}/access-token`, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
            ...authHeader,
        },
        body: JSON.stringify({ filename: path }),
    })

    if (!response.ok) {
        throw new Error(await response.text())
    }

    const token = (await response.json()) as FileAccessResponse

    return fileUrl(channelId, path, token.access)
}
