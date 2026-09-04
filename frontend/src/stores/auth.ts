import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'
import { authFetch } from '@/composables/authFetch'
import { useIndex } from '@/stores/index'

type LoginResult = {
    status: number
    verificationRequired: boolean
}

type AuthChannelMessage = { type: 'tokens-updated'; access: string; refresh: string } | { type: 'logout' }

const AUTH_CHANNEL_NAME = 'ffplayout-auth'

let refreshRequest: Promise<boolean> | null = null
let authChannel: BroadcastChannel | null = null

function broadcastAuthMessage(message: AuthChannelMessage) {
    authChannel?.postMessage(message)
}

function isAuthChannelMessage(message: unknown): message is AuthChannelMessage {
    if (!message || typeof message !== 'object' || !('type' in message)) {
        return false
    }

    if (message.type === 'logout') {
        return true
    }

    return (
        message.type === 'tokens-updated' &&
        'access' in message &&
        typeof message.access === 'string' &&
        'refresh' in message &&
        typeof message.refresh === 'string'
    )
}

/** Starts tab-to-tab synchronization after Pinia has been installed. */
export function initAuthChannel() {
    if (authChannel || typeof BroadcastChannel === 'undefined') {
        return
    }

    authChannel = new BroadcastChannel(AUTH_CHANNEL_NAME)
    authChannel.addEventListener('message', (event: MessageEvent<unknown>) => {
        if (!isAuthChannelMessage(event.data)) {
            return
        }

        const auth = useAuth()
        if (event.data.type === 'logout') {
            auth.removeToken(false)
            return
        }

        try {
            auth.updateToken(event.data.access, event.data.refresh, false)
        } catch {
            auth.removeToken(false)
        }
    })
}

export const useAuth = defineStore('auth', {
    state: () => ({
        isLogin: false,
        verificationPending: false,
        jwtToken: '',
        jwtRefresh: '',
        authHeader: {},
        id: 0,
        username: '',
        role: '',
        uuid: null as null | string,
    }),

    getters: {},
    actions: {
        updateToken(token: string, refresh: string, broadcast: boolean = true) {
            const decodedToken = jwtDecode<JwtPayloadExt>(token)
            const decodedRefresh = jwtDecode<JwtPayloadExt>(refresh)
            if (decodedToken.token_type !== 'access' || decodedRefresh.token_type !== 'refresh') {
                throw new Error('Invalid token types')
            }

            localStorage.setItem('token', token)
            localStorage.setItem('refresh', refresh)

            this.isLogin = true
            this.verificationPending = false
            this.jwtToken = token
            this.jwtRefresh = refresh
            this.authHeader = { Authorization: `Bearer ${token}` }
            this.id = decodedToken.id
            this.role = decodedToken.role

            if (broadcast) {
                broadcastAuthMessage({ type: 'tokens-updated', access: token, refresh })
            }
        },

        removeToken(broadcast: boolean = true) {
            localStorage.removeItem('token')
            localStorage.removeItem('refresh')

            this.isLogin = false
            this.jwtToken = ''
            this.jwtRefresh = ''
            this.authHeader = {}
            this.id = 0
            this.role = ''
            this.uuid = null

            if (broadcast) {
                broadcastAuthMessage({ type: 'logout' })
            }
        },

        async logout() {
            const refresh = this.jwtRefresh || localStorage.getItem('refresh') || ''
            this.removeToken()
            this.cancelVerification()

            if (!refresh) return

            try {
                await fetch('/auth/logout', {
                    method: 'POST',
                    headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                    body: JSON.stringify({ refresh }),
                })
            } catch {
                // Local logout must still succeed while the backend is unavailable.
            }
        },

        beginVerification() {
            // A previous session must not redirect the pending two-factor
            // login to the authenticated part of the application.
            this.removeToken(false)
            this.verificationPending = true
        },

        cancelVerification() {
            this.verificationPending = false
        },

        async obtainVerificationCode(password: string): Promise<LoginResult> {
            const payload = {
                username: this.username,
                password,
            }

            try {
                const response = await fetch('/auth/login', {
                    method: 'POST',
                    headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                    body: JSON.stringify(payload),
                })
                const data = (await response.json()) as Partial<Token>

                if (!response.ok) {
                    return { status: response.status, verificationRequired: false }
                }
                if (data.access && data.refresh) {
                    this.updateToken(data.access, data.refresh)
                    return { status: response.status, verificationRequired: false }
                }

                return { status: response.status, verificationRequired: true }
            } catch {
                return { status: 400, verificationRequired: false }
            }
        },

        async verifyCode(verificationCode: string) {
            const payload = {
                username: this.username,
                code: verificationCode,
            }

            try {
                const response = await fetch('/auth/verify', {
                    method: 'POST',
                    headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                    body: JSON.stringify(payload),
                })

                if (!response.ok) {
                    return response.status
                }

                const data = (await response.json()) as Partial<Token>
                if (!data.access || !data.refresh) {
                    return 400
                }

                this.updateToken(data.access, data.refresh)
                return response.status
            } catch {
                return 400
            }
        },

        async refreshToken(): Promise<boolean> {
            if (refreshRequest) {
                return refreshRequest
            }

            refreshRequest = (async () => {
                try {
                    const response = await fetch('/auth/refresh', {
                        method: 'POST',
                        headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                        body: JSON.stringify({ refresh: this.jwtRefresh }),
                    })
                    if (!response.ok) {
                        this.removeToken()
                        return false
                    }

                    const data = (await response.json()) as Partial<Token>
                    if (!data.access || !data.refresh) {
                        this.removeToken()
                        return false
                    }

                    this.updateToken(data.access, data.refresh)
                    return true
                } catch {
                    this.removeToken()
                    return false
                }
            })()

            try {
                return await refreshRequest
            } finally {
                refreshRequest = null
            }
        },

        async inspectToken() {
            const token = localStorage.getItem('token')
            const refresh = localStorage.getItem('refresh')

            if (!token || !refresh) {
                this.removeToken()
                return
            }

            try {
                const decodedToken = jwtDecode<JwtPayloadExt>(token)
                const decodedRefresh = jwtDecode<JwtPayloadExt>(refresh)

                if (decodedToken.token_type !== 'access' || decodedRefresh.token_type !== 'refresh') {
                    this.removeToken()
                    return
                }

                this.jwtToken = token
                this.jwtRefresh = refresh
                this.authHeader = { Authorization: `Bearer ${token}` }
                this.id = decodedToken.id
                this.role = decodedToken.role

                const timestamp = Date.now() / 1000
                const expireToken = decodedToken.exp || 0
                const expireRefresh = decodedRefresh.exp || 0

                if (expireToken - timestamp > 15) {
                    this.isLogin = true
                    return
                }
                if (expireRefresh - timestamp > 0) {
                    await this.refreshToken()
                    return
                }

                this.removeToken()
            } catch {
                this.removeToken()
            }
        },

        async selectAuthUser() {
            const store = useIndex()
            await authFetch<User>(`/api/user/${this.id}`)
                .then((response: any) => {
                    if (response) {
                        this.id = response.id
                        this.username = response.username
                    }
                })
                .catch((e) => {
                    store.msgAlert('error', e)
                })
        },

        async obtainUuid() {
            await authFetch<{ uuid: string }>('/api/generate-uuid', { method: 'POST' })
                .then((response: any) => {
                    this.uuid = response.uuid
                })
                .catch((e) => {
                    if (e.status === 401) {
                        this.removeToken()
                    }
                    this.uuid = null
                })
        },
    },
})
