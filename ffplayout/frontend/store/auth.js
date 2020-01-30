/* eslint-disable camelcase */
import jwt_decode from 'jwt-decode'

export const state = () => ({
    jwtToken: localStorage.getItem('token'),
    jwtRefresh: localStorage.getItem('refresh'),
    isLogin: false
})

// mutate values in state
export const mutations = {
    UPADTE_TOKEN (state, obj) {
        localStorage.setItem('token', obj.token)
        state.jwtToken = obj.token

        if (obj.refresh) {
            localStorage.setItem('refresh', obj.refresh)
            state.jwtRefresh = obj.refresh
        }
    },
    REMOVE_TOKEN (state) {
        localStorage.removeItem('token')
        localStorage.removeItem('refresh')
        state.jwtToken = null
        state.jwtRefresh = null
    },
    UPDATE_IS_LOGIN (state, bool) {
        state.isLogin = bool
    }
}

export const actions = {
    async obtainToken ({ commit, state }, { username, password }) {
        const payload = {
            username,
            password
        }
        await this.$axios.post('auth/token/', payload)
            .then((response) => {
                commit('UPADTE_TOKEN', { token: response.data.access, refresh: response.data.refresh })
                commit('UPDATE_IS_LOGIN', true)
            })
            .catch((error) => {
                console.log(error)
            })
    },
    async refreshToken ({ commit, state }) {
        const payload = {
            refresh: state.jwtRefresh,
            progress: false
        }
        const response = await this.$axios.post('auth/token/refresh/', payload)

        commit('UPADTE_TOKEN', { token: response.data.access })
        commit('UPDATE_IS_LOGIN', true)
    },

    async inspectToken ({ commit, dispatch, state }) {
        const token = state.jwtToken
        const refresh = state.jwtRefresh
        if (token && refresh) {
            const decoded_token = jwt_decode(token)
            const decoded_refresh = jwt_decode(refresh)
            const timestamp = Date.now() / 1000
            const expire_token = decoded_token.exp
            const expire_refresh = decoded_refresh.exp
            if (expire_token - timestamp > 0) {
                // DO NOTHING, DO NOT REFRESH
                commit('UPDATE_IS_LOGIN', true)
            } else if (expire_refresh - timestamp > 0) {
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
