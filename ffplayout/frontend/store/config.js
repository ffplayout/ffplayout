export const state = () => ({
    configGui: null,
    netChoices: [],
    configPlayout: null,
    currentUser: null,
    configUser: null
})

export const mutations = {
    UPDATE_GUI_CONFIG (state, config) {
        state.configGui = config
    },
    UPDATE_NET_CHOICES (state, list) {
        state.netChoices = list
    },
    UPDATE_PLAYLOUT_CONFIG (state, config) {
        state.configPlayout = config
    },
    SET_CURRENT_USER (state, user) {
        state.currentUser = user
    },
    UPDATE_USER_CONFIG (state, config) {
        state.configUser = config
    }
}

export const actions = {
    async getGuiConfig ({ commit, state, rootState }) {
        const options = await this.$axios.options('api/guisettings/', { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        const response = await this.$axios.get('api/guisettings/', { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        if (options.data) {
            const choices = options.data.actions.POST.net_interface.choices.map(function (obj) {
                obj.text = obj.display_name
                delete obj.display_name
                return obj
            })
            commit('UPDATE_NET_CHOICES', choices)
        }
        if (response.data) {
            response.data[0].extra_extensions = response.data[0].extra_extensions.split(' ')
            commit('UPDATE_GUI_CONFIG', response.data[0])
        }
    },

    async setGuiConfig ({ commit, state, rootState }, obj) {
        const stringObj = JSON.parse(JSON.stringify(obj))
        stringObj.extra_extensions = obj.extra_extensions.join(' ')
        const update = await this.$axios.put('api/guisettings/1/', stringObj, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        return update
    },

    async getPlayoutConfig ({ commit, state, rootState }) {
        const response = await this.$axios.get('api/config/?configPlayout', { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        if (response.data) {
            commit('UPDATE_PLAYLOUT_CONFIG', response.data)
        }
    },

    async setPlayoutConfig ({ commit, state, rootState }, obj) {
        const update = await this.$axios.post('api/config/?configPlayout', { data: obj }, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        return update
    },

    async getUserConfig ({ commit, state, rootState }) {
        const user = await this.$axios.get('api/current/user/', { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        const response = await this.$axios.get(`api/users/?username=${user.data.username}`, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        if (user.data) {
            commit('SET_CURRENT_USER', user.data.username)
        }
        if (response.data) {
            commit('UPDATE_USER_CONFIG', response.data[0])
        }
    },

    async setUserConfig ({ commit, state, rootState }, obj) {
        const update = await this.$axios.put(`api/users/${obj.id}/`, obj, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })
        return update
    }
}
