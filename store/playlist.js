import _ from 'lodash'

export const state = () => ({
    playlist: null,
    playlistToday: [],
    progressValue: 0,
    currentClip: 'No clips is playing',
    currentClipIndex: null,
    currentClipStart: null,
    currentClipDuration: null,
    currentClipIn: null,
    currentClipOut: null,
    timeStr: '00:00:00',
    timeLeft: '00:00:00'
})

export const mutations = {
    UPDATE_PLAYLIST (state, list) {
        state.playlist = list
    },
    UPDATE_TODAYS_PLAYLIST (state, list) {
        state.playlistToday = list
    },
    SET_PROGRESS_VALUE (state, value) {
        state.progressValue = value
    },
    SET_CURRENT_CLIP (state, clip) {
        state.currentClip = clip
    },
    SET_CURRENT_CLIP_INDEX (state, index) {
        state.currentClipIndex = index
    },
    SET_CURRENT_CLIP_START (state, start) {
        state.currentClipStart = start
    },
    SET_CURRENT_CLIP_DURATION (state, dur) {
        state.currentClipDuration = dur
    },
    SET_CURRENT_CLIP_IN (state, _in) {
        state.currentClipIn = _in
    },
    SET_CURRENT_CLIP_OUT (state, out) {
        state.currentClipOut = out
    },
    SET_TIME (state, time) {
        state.timeStr = time
    },
    SET_TIME_LEFT (state, time) {
        state.timeLeft = time
    }
}

export const actions = {
    async getPlaylist ({ commit, rootState }, { date }) {
        const timeInSec = this.$timeToSeconds(this.$dayjs().utcOffset(rootState.config.utcOffset).format('HH:mm:ss'))
        const channel = rootState.config.configGui[rootState.config.configID].id
        let dateToday = this.$dayjs().utcOffset(rootState.config.utcOffset).format('YYYY-MM-DD')

        if (rootState.config.startInSec > timeInSec) {
            dateToday = this.$dayjs(dateToday).utcOffset(rootState.config.utcOffset).subtract(1, 'day').format('YYYY-MM-DD')
        }

        const response = await this.$axios.get(`api/playlist/${channel}?date=${date}`)

        if (response.data && response.data.program) {
            commit('UPDATE_PLAYLIST', this.$processPlaylist(rootState.config.startInSec, rootState.config.playlistLength, response.data.program, false))

            if (date === dateToday) {
                commit('UPDATE_TODAYS_PLAYLIST', _.cloneDeep(response.data.program))
            } else {
                commit('SET_CURRENT_CLIP_INDEX', null)
            }
        } else {
            commit('UPDATE_PLAYLIST', [])
        }
    },

    async playoutStat ({ commit, state, rootState }) {
        const channel = rootState.config.configGui[rootState.config.configID].id
        const time = this.$dayjs().utcOffset(rootState.config.utcOffset).format('HH:mm:ss')
        let timeSec = this.$timeToSeconds(time)

        commit('SET_TIME', time)

        if (timeSec < rootState.config.startInSec) {
            timeSec += rootState.config.playlistLength
        }

        if (timeSec < state.currentClipStart) {
            return
        }

        const response = await this.$axios.get(`api/control/${channel}/media/current`)

        if (response.data && response.data.result && response.data.result.played_sec) {
            const obj = response.data.result
            const progValue = obj.played_sec * 100 / obj.current_media.out
            commit('SET_PROGRESS_VALUE', progValue)
            commit('SET_CURRENT_CLIP', obj.current_media.source)
            commit('SET_CURRENT_CLIP_INDEX', obj.index)
            commit('SET_CURRENT_CLIP_START', obj.start_sec)
            commit('SET_CURRENT_CLIP_DURATION', obj.current_media.duration)
            commit('SET_CURRENT_CLIP_IN', obj.current_media.seek)
            commit('SET_CURRENT_CLIP_OUT', obj.current_media.out)
            commit('SET_TIME_LEFT', this.$secToHMS(obj.remaining_sec))
        }
    }
}
