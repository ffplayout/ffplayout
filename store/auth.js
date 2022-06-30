/* eslint-disable camelcase */
import jwt_decode from 'jwt-decode'

export const state = () => ({
    jwtToken: '',
    isLogin: false
})

// mutate values in state
export const mutations = {
    UPDATE_TOKEN (state, obj) {
        state.jwtToken = obj.token
        this.$cookies.set('token', obj.token, {
            path: '/',
            maxAge: 60 * 60 * 24 * 365,
            sameSite: 'lax'
        })
    },
    UPDATE_IS_LOGIN (state, bool) {
        state.isLogin = bool
    },
    REMOVE_TOKEN (state) {
        this.$cookies.remove('token')
        state.jwtToken = null
    }
}

export const actions = {
    async obtainToken ({ commit, state }, { username, password }) {
        const payload = {
            username,
            password
        }
        let code = null
        await this.$axios.post('auth/login/', payload)
            .then((response) => {
                commit('UPDATE_TOKEN', { token: response.data.user.token })
                commit('UPDATE_IS_LOGIN', true)
                code = response.status
            })
            .catch((error) => {
                code = error.response.status
            })

        return code
    },

    inspectToken ({ commit, dispatch, state }) {
        const token = this.$cookies.get('token')

        if (token) {
            commit('UPDATE_TOKEN', { token })
            const decoded_token = jwt_decode(token)
            const timestamp = Date.now() / 1000
            const expire_token = decoded_token.exp

            if (state.jwtToken && expire_token - timestamp > 15) {
                commit('UPDATE_IS_LOGIN', true)
            } else {
                // PROMPT USER TO RE-LOGIN, THIS ELSE CLAUSE COVERS THE CONDITION WHERE A TOKEN IS EXPIRED AS WELL
                commit('UPDATE_IS_LOGIN', false)
            }
        } else {
            commit('UPDATE_IS_LOGIN', false)
        }
    }
}
