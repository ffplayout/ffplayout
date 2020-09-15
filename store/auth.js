/* eslint-disable camelcase */
import jwt_decode from 'jwt-decode'

export const state = () => ({
    jwtToken: '',
    jwtRefresh: '',
    isLogin: false
})

// mutate values in state
export const mutations = {
    UPADTE_TOKEN (state, obj) {
        state.jwtToken = obj.token
        this.$cookies.set('token', obj.token, {
            path: '/',
            maxAge: 60 * 60 * 24 * 365,
            sameSite: 'lax'
        })

        if (obj.refresh) {
            state.jwtRefresh = obj.refresh
            this.$cookies.set('refresh', obj.refresh, {
                path: '/',
                maxAge: 60 * 60 * 24 * 365,
                sameSite: 'lax'
            })
        }
    },
    UPDATE_IS_LOGIN (state, bool) {
        state.isLogin = bool
    },
    REMOVE_TOKEN (state) {
        this.$cookies.remove('token')
        this.$cookies.remove('refresh')
        state.jwtToken = null
        state.jwtRefresh = null
    }
}

export const actions = {
    async obtainToken ({ commit, state }, { username, password }) {
        const payload = {
            username,
            password
        }
        let code = null
        await this.$axios.post('auth/token/', payload)
            .then((response) => {
                commit('UPADTE_TOKEN', { token: response.data.access, refresh: response.data.refresh })
                commit('UPDATE_IS_LOGIN', true)
                code = response.status
            })
            .catch((error) => {
                code = error.response.status
            })

        return code
    },
    async refreshToken ({ commit, state }) {
        const payload = {
            refresh: state.jwtRefresh,
            progress: false
        }

        await this.$axios.post('auth/token/refresh/', payload)
            .then((response) => {
                commit('UPADTE_TOKEN', { token: response.data.access })
                commit('UPDATE_IS_LOGIN', true)
            })
            .catch((error) => {
                if (error.response.status === 401) {
                    commit('REMOVE_TOKEN')
                    commit('UPDATE_IS_LOGIN', false)
                }
            })
    },

    async inspectToken ({ commit, dispatch, state }) {
        const token = this.$cookies.get('token')
        const refresh = this.$cookies.get('refresh')

        if (token && refresh) {
            commit('UPADTE_TOKEN', { token, refresh })
            const decoded_token = jwt_decode(token)
            const decoded_refresh = jwt_decode(refresh)
            const timestamp = Date.now() / 1000
            const expire_token = decoded_token.exp
            const expire_refresh = decoded_refresh.exp

            if (state.jwtToken && expire_token - timestamp > 15) {
                // DO NOTHING, DO NOT REFRESH
                commit('UPDATE_IS_LOGIN', true)
            } else if (!state.jwtToken || expire_refresh - timestamp > 0) {
                commit('UPADTE_TOKEN', { token, refresh })
                await dispatch('refreshToken')
            } else {
                // PROMPT USER TO RE-LOGIN, THIS ELSE CLAUSE COVERS THE CONDITION WHERE A TOKEN IS EXPIRED AS WELL
                commit('UPDATE_IS_LOGIN', false)
            }
        } else {
            commit('UPDATE_IS_LOGIN', false)
        }
    }
}
