export const state = () => ({
    configID: 0,
    configCount: 0,
    configGui: null,
    netChoices: [],
    configPlayout: [],
    currentUser: null,
    configUser: null
})

export const mutations = {
    UPDATE_CONFIG_ID (state, id) {
        state.configID = id
    },
    UPDATE_CONFIG_COUNT (state, count) {
        state.configCount = count
    },
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
    async nuxtClientInit ({ commit, dispatch, rootState }) {
        await dispatch('auth/inspectToken', null, { root: true })
        if (rootState.auth.isLogin) {
            await dispatch('getGuiConfig')
            await dispatch('getPlayoutConfig')
            await dispatch('getUserConfig')
        }
    },

    async getGuiConfig ({ commit, state }) {
        const options = await this.$axios.options('api/player/guisettings/')
        const response = await this.$axios.get('api/player/guisettings/')

        if (options.data) {
            const choices = options.data.actions.POST.net_interface.choices.map(function (obj) {
                obj.text = obj.display_name
                delete obj.display_name
                return obj
            })
            commit('UPDATE_NET_CHOICES', choices)
        }

        if (response.data && response.data[0]) {
            for (const data of response.data) {
                if (data.extra_extensions) {
                    data.extra_extensions = data.extra_extensions.split(',')
                } else {
                    data.extra_extensions = []
                }
            }

            commit('UPDATE_GUI_CONFIG', response.data)
            commit('UPDATE_CONFIG_COUNT', response.data.length)
        } else {
            commit('UPDATE_GUI_CONFIG', [{
                id: 1,
                channel: '',
                player_url: '',
                playout_config: '',
                net_interface: '',
                media_disk: '',
                extra_extensions: []
            }])
        }
    },

    async setGuiConfig ({ commit, state }, obj) {
        const stringObj = JSON.parse(JSON.stringify(obj[state.configID]))
        stringObj.extra_extensions = stringObj.extra_extensions.join(',')
        let response

        if (state.configPlayout.length === 0 || state.configCount !== stringObj.length) {
            response = await this.$axios.post('api/player/guisettings/', stringObj)
        } else {
            response = await this.$axios.put(`api/player/guisettings/${obj[state.configID].id}/`, stringObj)
        }

        return response
    },

    async getPlayoutConfig ({ commit, state }) {
        const response = await this.$axios.get('api/player/config/?configPlayout')

        if (response.data) {
            commit('UPDATE_PLAYLOUT_CONFIG', response.data)
        }
    },

    async setPlayoutConfig ({ commit, state }, obj) {
        const update = await this.$axios.post('api/player/config/?configPlayout', { data: obj })
        return update
    },

    async getUserConfig ({ commit, state }) {
        const user = await this.$axios.get('api/player/user/current/')
        const response = await this.$axios.get(`api/player/user/users/?username=${user.data.username}`)

        if (user.data) {
            commit('SET_CURRENT_USER', user.data.username)
        }
        if (response.data) {
            commit('UPDATE_USER_CONFIG', response.data[0])
        }
    },

    async setUserConfig ({ commit, state }, obj) {
        const update = await this.$axios.put(`api/player/user/users/${obj.id}/`, obj)
        return update
    }
}
