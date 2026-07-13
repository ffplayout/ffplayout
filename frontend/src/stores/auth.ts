import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'
import { useIndex } from '@/stores/index'

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

        async obtainVerificationCode(password: string) {
            let code = 400

            const payload = {
                username: this.username,
                password,
            }

            await fetch('/auth/login', {
                method: 'POST',
                headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                body: JSON.stringify(payload),
            })
                .then((resp) => {
                    code = resp.status
                    return resp.json()
                })
                .then((response: Token) => {
                    if (response?.access) {
                        this.updateToken(response.access, response.refresh)
                    }
                })
                .catch((e) => {
                    code = typeof e.status === 'number' ? e.status : code
                })

            return code
        },

        async verifyCode(verificationCode: string) {
            let code = 400

            const payload = {
                username: this.username,
                code: verificationCode,
            }

            await fetch('/auth/verify', {
                method: 'POST',
                headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                body: JSON.stringify(payload),
            })
                .then((resp) => {
                    code = resp.status

                    if (code === 200) {
                        return resp.json()
                    }
                })
                .then((response: Token) => {
                    if (response?.access) {
                        this.updateToken(response.access, response.refresh)
                    }
                })
                .catch((e) => {
                    code = typeof e.status === 'number' ? e.status : code
                })

            return code
        },

        async refreshToken() {
            await fetch('/auth/refresh', {
                method: 'POST',
                headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                body: JSON.stringify({ refresh: this.jwtRefresh }),
            })
                .then((resp) => resp.json())
                .then((response: any) => {
                    if (response.access) {
                        this.updateToken(response.access, this.jwtRefresh)
                    }
                })
                .catch(() => {
                    this.removeToken()
                })
        },

        async inspectToken() {
            const token = localStorage.getItem('token')
            const refresh = localStorage.getItem('refresh')

            if (token && refresh) {
                this.jwtToken = token
                this.jwtRefresh = refresh
                this.authHeader = { Authorization: `Bearer ${token}` }

                const decodedToken = jwtDecode<JwtPayloadExt>(token)
                const decodedRefresh = jwtDecode<JwtPayloadExt>(refresh)
                const timestamp = Date.now() / 1000
                const expireToken = decodedToken.exp
                const expireRefresh = decodedRefresh.exp || 0

                this.id = decodedToken.id
                this.role = decodedToken.role

                if (expireToken && this.jwtToken && expireToken - timestamp > 15) {
                    this.isLogin = true
                } else if (expireRefresh && expireRefresh - timestamp > 0) {
                    await this.refreshToken()
                } else {
                    // Prompt user to re-login.
                    this.removeToken()
                }
            } else {
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
