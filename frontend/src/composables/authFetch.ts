import { useAuth } from '@/stores/auth'

type AuthResponseType = 'json' | 'text' | 'blob' | 'arrayBuffer'

export type AuthFetchOptions = RequestInit & {
    responseType?: AuthResponseType
}

export class AuthFetchError<T = unknown> extends Error {
    constructor(
        public readonly response: Response,
        public readonly data: T,
    ) {
        super(fetchErrorMessage(response, data))
    }

    get status(): number {
        return this.response.status
    }
}

function fetchErrorMessage(response: Response, data: unknown): string {
    if (typeof data === 'string' && data) {
        return data
    }
    if (data && typeof data === 'object') {
        const detail = 'detail' in data && typeof data.detail === 'string' ? data.detail : undefined
        const error = 'error' in data && typeof data.error === 'string' ? data.error : undefined

        if (detail || error) {
            return detail ?? error ?? `Request failed with status ${response.status}`
        }
    }

    return `Request failed with status ${response.status}`
}

/**
 * Sends an authenticated request and returns its parsed response body.
 */
export async function authFetch<T = unknown>(
    input: RequestInfo | URL,
    { responseType, ...init }: AuthFetchOptions = {},
): Promise<T> {
    const response = await authFetchRaw(input, init)
    const type = responseType ?? defaultResponseType(response)

    if (!response.ok) {
        throw new AuthFetchError(response, await readResponse(response, type))
    }
    if (response.status === 204) {
        return undefined as T
    }

    return (await readResponse(response, type)) as T
}

/**
 * Sends an authenticated request and returns the unparsed response. Use this
 * for downloads and callers that need headers or streaming response bodies.
 */
export async function authFetchRaw(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
    const auth = useAuth()

    await auth.inspectToken()
    if (!auth.isLogin) {
        return new Response(null, { status: 401, statusText: 'Authentication required' })
    }

    const request = new Request(input, init)
    const send = async () => {
        const headers = new Headers(request.headers)
        headers.set('Authorization', `Bearer ${auth.jwtToken}`)

        return fetch(new Request(request.clone(), { headers }))
    }

    const accessToken = auth.jwtToken
    let response = await send()
    if (response.status !== 401) {
        return response
    }

    // Another request may have completed the shared refresh while this one
    // was in flight. In that case retry with its already-current access token.
    if (auth.jwtToken === accessToken && !(await auth.refreshToken())) {
        return response
    }

    response = await send()
    return response
}

function defaultResponseType(response: Response): AuthResponseType {
    return response.headers.get('content-type')?.includes('application/json') ? 'json' : 'text'
}

async function readResponse(response: Response, type: AuthResponseType): Promise<unknown> {
    switch (type) {
        case 'json':
            return response.json()
        case 'blob':
            return response.blob()
        case 'arrayBuffer':
            return response.arrayBuffer()
        case 'text':
            return response.text()
    }
}
