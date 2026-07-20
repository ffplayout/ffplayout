import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'
import { useIndex } from '@/stores/index'

type LoginResult = {
    status: number
    verificationRequired: boolean
}

let refreshRequest: Promise<boolean> | null = null

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
        updateToken(token: string, refresh: string) {
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
        },

        removeToken() {
            localStorage.removeItem('token')
            localStorage.removeItem('refresh')

            this.isLogin = false
            this.jwtToken = ''
            this.jwtRefresh = ''
            this.authHeader = {}
            this.id = 0
            this.role = ''
            this.uuid = null
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
            this.removeToken()
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
            await fetch(`/api/user/${this.id}`, {
                headers: this.authHeader,
            })
                .then(async (resp) => {
                    if (resp.status >= 400) {
                        const msg = (await resp.json())?.error ?? (await resp.text())

                        if (msg.includes('Unauthorized')) {
                            this.removeToken()
                        }
                        throw new Error(msg)
                    }
                    return resp.json()
                })
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
            await fetch('/api/generate-uuid', {
                method: 'POST',
                headers: this.authHeader,
            })
                .then(async (resp) => {
                    if (!resp.ok) {
                        if (resp.status === 401) {
                            this.removeToken()
                        }
                        this.uuid = null
                    }

                    return resp.json()
                })
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
