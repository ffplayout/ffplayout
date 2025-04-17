import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'

export const useAuth = defineStore('auth', {
    state: () => ({
        isLogin: false,
        jwtToken: '',
        jwtRefresh: '',
        authHeader: {},
        role: '',
        uuid: null as null | string,
    }),

    getters: {},
    actions: {
        updateToken(token: string, refresh: string) {
            localStorage.setItem('token', token)
            localStorage.setItem('refresh', refresh)

            this.jwtToken = token
            this.jwtRefresh = refresh
            this.authHeader = { Authorization: `Bearer ${token}` }
        },

        removeToken() {
            localStorage.removeItem('token')
            localStorage.removeItem('refresh')

            this.isLogin = false
            this.jwtToken = ''
            this.jwtRefresh = ''
            this.authHeader = {}
        },

        async obtainToken(username: string, password: string) {
            let code = 400
            const payload = {
                username,
                password,
            }

            await fetch('/auth/login/', {
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
                        const decodedToken = jwtDecode<JwtPayloadExt>(response.access)
                        this.isLogin = true
                        this.role = decodedToken.role
                    }
                })
                .catch((e) => {
                    code = typeof e.status === 'number' ? e.status : code
                })

            return code
        },

        async obtainUuid() {
            await fetch('/api/generate-uuid', {
                method: 'POST',
                headers: this.authHeader,
            })
                .then((resp) => resp.json())
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

        async refreshToken() {
            await fetch('/auth/refresh/', {
                method: 'POST',
                headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                body: JSON.stringify({ refresh: this.jwtRefresh }),
            })
                .then((resp) => resp.json())
                .then((response: any) => {
                    if (response.access) {
                        this.updateToken(response.access, this.jwtRefresh)
                        this.isLogin = true
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
    },
})
