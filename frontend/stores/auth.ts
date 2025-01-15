import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'

export const useAuth = defineStore('auth', {
    state: () => ({
        isLogin: false,
        jwtToken: '',
        authHeader: {},
        channelID: 0,
        role: '',
        uuid: null as null | string,
    }),

    getters: {},
    actions: {
        updateToken(token: string) {
            const cookie = useCookie('token', {
                path: '/',
                maxAge: 60 * 60 * 24 * 365,
                sameSite: 'lax',
            })

            cookie.value = token
            this.jwtToken = token
            this.authHeader = { Authorization: `Bearer ${token}` }
        },

        removeToken() {
            const cookie = useCookie('token')
            cookie.value = null
            this.isLogin = false
            this.jwtToken = ''
            this.authHeader = {}
        },

        async obtainToken(username: string, password: string) {
            let code = 0
            const payload = {
                username,
                password,
            }

            await $fetch('/auth/login/', {
                method: 'POST',
                body: JSON.stringify(payload),
                async onResponse(data: any) {
                    code = data.response.status
                },
            })
                .then((response: any) => {
                    this.updateToken(response.user?.token)
                    const decodedToken = jwtDecode<JwtPayloadExt>(response.user?.token)
                    this.isLogin = true
                    this.channelID = decodedToken.channel
                    this.role = decodedToken.role
                })
                .catch((e) => {
                    code = e.status
                })

            return code
        },

        async obtainUuid() {
            await $fetch('/api/generate-uuid', {
                method: 'POST',
                headers: this.authHeader,
            })
                .then((response: any) => {
                    this.uuid = response.uuid
                })
                .catch(e => {
                    if (e.status === 401) {
                        this.removeToken()
                    }
                    this.uuid = null
                })
        },

        inspectToken() {
            const token = useCookie('token').value

            if (token) {
                this.updateToken(token)
                const decodedToken = jwtDecode<JwtPayloadExt>(token)
                const timestamp = Date.now() / 1000
                const expireToken = decodedToken.exp
                this.channelID = decodedToken.channel
                this.role = decodedToken.role

                if (expireToken && this.jwtToken && expireToken - timestamp > 15) {
                    this.isLogin = true
                } else {
                    // Prompt user to re-login.
                    this.isLogin = false
                }
            } else {
                this.isLogin = false
            }
        },
    },
})
