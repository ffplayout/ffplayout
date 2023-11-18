import { defineStore } from 'pinia'
import { jwtDecode } from 'jwt-decode'
import { type JwtPayload } from 'jwt-decode'

interface JwtPayloadExt extends JwtPayload {
    role: string
}

export const useAuth = defineStore('auth', {
    state: () => ({
        isLogin: false,
        jwtToken: '',
        authHeader: {},
        role: '',
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
            this.jwtToken = ''
            this.authHeader = {}
        },

        async obtainToken(username: string, password: string) {
            let code = 0
            const payload = {
                username,
                password,
            }

            await fetch('/auth/login/', {
                method: 'POST',
                headers: new Headers([['content-type', 'application/json;charset=UTF-8']]),
                body: JSON.stringify(payload),
            })
                .then((response) => {
                    code = response.status
                    return response
                })
                .then((response) => response.json())
                .then((response) => {
                    this.updateToken(response.user.token)
                    const decodedToken = jwtDecode<JwtPayloadExt>(response.user.token)
                    this.isLogin = true
                    this.role = decodedToken.role
                })
                .catch((error) => {
                    if (error.status) {
                        code = error.status
                    }
                })

            return code
        },

        inspectToken() {
            let token = useCookie('token').value

            if (token === null) {
                token = ''
            }

            if (token) {
                this.updateToken(token)
                const decodedToken = jwtDecode<JwtPayloadExt>(token)
                const timestamp = Date.now() / 1000
                const expireToken = decodedToken.exp
                this.role = decodedToken.role

                if (expireToken && this.jwtToken && expireToken - timestamp > 15) {
                    this.isLogin = true
                } else {
                    // Prompt user to re login.
                    this.isLogin = false
                }
            } else {
                this.isLogin = false
            }
        },
    },
})
