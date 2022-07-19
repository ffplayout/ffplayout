import _ from 'lodash'

export const state = () => ({
    configID: 0,
    configCount: 0,
    configGui: null,
    configGuiRaw: null,
    startInSec: 0,
    playlistLength: 86400.0,
    configPlayout: {},
    currentUser: null,
    configUser: null,
    timezone: 'UTC'
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
    UPDATE_GUI_CONFIG_RAW (state, config) {
        state.configGuiRaw = config
    },
    UPDATE_START_TIME (state, sec) {
        state.startInSec = sec
    },
    UPDATE_PLAYLIST_LENGTH (state, sec) {
        state.playlistLength = sec
    },
    UPDATE_PLAYOUT_CONFIG (state, config) {
        state.configPlayout = config
    },
    SET_CURRENT_USER (state, user) {
        state.currentUser = user
    },
    UPDATE_USER_CONFIG (state, config) {
        state.configUser = config
    },
    UPDATE_TIMEZONE (state, zone) {
        state.timezone = zone
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
        const response = await this.$axios.get('api/channels')

        if (response.data) {
            for (const data of response.data) {
                if (data.extra_extensions) {
                    data.extra_extensions = data.extra_extensions.split(',')
                } else {
                    data.extra_extensions = []
                }
            }

            commit('UPDATE_TIMEZONE', response.data.timezone)
            commit('UPDATE_GUI_CONFIG', response.data)
            commit('UPDATE_GUI_CONFIG_RAW', _.cloneDeep(response.data))
            commit('UPDATE_CONFIG_COUNT', response.data.length)
        } else {
            commit('UPDATE_GUI_CONFIG', [{
                id: 1,
                channel: '',
                preview_url: '',
                playout_config: '',
                extra_extensions: []
            }])
        }
    },

    async setGuiConfig ({ commit, state, dispatch }, obj) {
        const stringObj = _.cloneDeep(obj)
        stringObj.extra_extensions = stringObj.extra_extensions.join(',')
        let response

        if (state.configGuiRaw.some(e => e.id === stringObj.id)) {
            response = await this.$axios.patch(`api/channel/${obj.id}`, stringObj)
        } else {
            response = await this.$axios.post('api/channel/', stringObj)
            const guiConfigs = []

            for (const obj of state.configGui) {
                if (obj.name === stringObj.name) {
                    response.data.extra_extensions = response.data.extra_extensions.split(',')
                    guiConfigs.push(response.data)
                } else {
                    guiConfigs.push(obj)
                }
            }

            commit('UPDATE_GUI_CONFIG', guiConfigs)
            commit('UPDATE_GUI_CONFIG_RAW', _.cloneDeep(guiConfigs))
            commit('UPDATE_CONFIG_COUNT', guiConfigs.length)

            await dispatch('getPlayoutConfig')
        }

        return response
    },

    async getPlayoutConfig ({ commit, state, rootState }) {
        const channel = state.configGui[state.configID].id
        const response = await this.$axios.get(`api/playout/config/${channel}`)

        if (response.data) {
            if (response.data.playlist.day_start) {
                commit('UPDATE_START_TIME', this.$timeToSeconds(response.data.playlist.day_start))
            }

            if (response.data.playlist.length) {
                commit('UPDATE_PLAYLIST_LENGTH', this.$timeToSeconds(response.data.playlist.length))
            }

            commit('UPDATE_PLAYOUT_CONFIG', response.data)
        } else {
            rootState.showErrorAlert = true
            rootState.ErrorAlertMessage = 'No playout config found!'
        }
    },

    async setPlayoutConfig ({ commit, state }, obj) {
        const channel = state.configGui[state.configID].id
        const update = await this.$axios.put(`api/playout/config/${channel}`, obj)
        return update
    },

    async getUserConfig ({ commit, state }) {
        const user = await this.$axios.get('api/user')

        if (user.data) {
            commit('SET_CURRENT_USER', user.data.username)
        }
        if (user.data) {
            commit('UPDATE_USER_CONFIG', user.data)
        }
    },

    async setUserConfig ({ commit, state }, obj) {
        const update = await this.$axios.put(`api/user/${obj.id}`, obj)
        return update
    }
}
