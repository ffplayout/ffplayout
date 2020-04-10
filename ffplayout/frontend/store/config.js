export const state = () => ({
    config: null
})

export const mutations = {
    UPDATE_CONFIG (state, config) {
        state.config = config
    }
}

export const actions = {
    async getConfig ({ commit, state, rootState }) {
        const response = await this.$axios.get('api/config/?config', { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        if (response.data) {
            commit('UPDATE_CONFIG', response.data)
        }
    },

    async setConfig ({ commit, state, rootState }, obj) {
        const response = await this.$axios.post('api/config/?config', { data: obj }, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        console.log(response)
    }
}
