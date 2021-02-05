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
    async getPlaylist ({ commit, dispatch, state, rootState }, { dayStart, date }) {
        const timeInSec = this.$timeToSeconds(this.$dayjs().format('HH:mm:ss'))
        let dateToday = this.$dayjs().format('YYYY-MM-DD')

        if (rootState.config.startInSec > timeInSec) {
            dateToday = this.$dayjs(dateToday).subtract(1, 'day').format('YYYY-MM-DD')
        }

        const response = await this.$axios.get(`api/player/playlist/?date=${date}`)

        if (response.data && response.data.program) {
            commit('UPDATE_PLAYLIST', this.$processPlaylist(dayStart, response.data.program))

            if (date === dateToday) {
                commit('UPDATE_TODAYS_PLAYLIST', JSON.parse(JSON.stringify(response.data.program)))
                dispatch('setCurrentClip')
            } else {
                commit('SET_CURRENT_CLIP_INDEX', null)
            }
        } else {
            commit('UPDATE_PLAYLIST', [])
        }
    },

    setCurrentClip ({ commit, dispatch, state, rootState }) {
        let begin
        let lastTime = this.$timeToSeconds(this.$dayjs().format('HH:mm:ss'))

        if (Number.isFinite(rootState.config.startInSec)) {
            begin = rootState.config.startInSec
        } else {
            commit('SET_CURRENT_CLIP', 'day_start is not set, cannot calculate current clip')
            return
        }

        if (lastTime < begin) {
            lastTime += rootState.config.playlistLength
        }

        for (let i = 0; i < state.playlistToday.length; i++) {
            const duration = state.playlistToday[i].out - state.playlistToday[i].in

            // animate the progress bar
            if (lastTime < begin + duration) {
                const progValue = (lastTime - begin) * 100 / duration
                commit('SET_PROGRESS_VALUE', progValue)
                commit('SET_CURRENT_CLIP', state.playlistToday[i].source)
                commit('SET_CURRENT_CLIP_INDEX', i)
                commit('SET_CURRENT_CLIP_START', begin)
                commit('SET_CURRENT_CLIP_DURATION', duration)
                commit('SET_CURRENT_CLIP_IN', state.playlistToday[i].in)
                commit('SET_CURRENT_CLIP_OUT', state.playlistToday[i].out)

                break
            }

            begin += duration
        }
    },

    animClock ({ commit, dispatch, state, rootState }) {
        const time = this.$dayjs().format('HH:mm:ss')
        let timeSec = this.$timeToSeconds(time)

        commit('SET_TIME', time)

        if (timeSec < rootState.config.startInSec) {
            timeSec += rootState.config.playlistLength
        }

        if (timeSec < state.currentClipStart) {
            return
        }

        const playTime = timeSec - state.currentClipStart
        const progValue = playTime * 100 / state.currentClipDuration

        // set progress bar value
        if (playTime <= state.currentClipDuration && progValue >= 0) {
            commit('SET_PROGRESS_VALUE', progValue)
            commit('SET_TIME_LEFT', this.$secToHMS(state.currentClipDuration - playTime))
        } else {
            commit('SET_PROGRESS_VALUE', 0)
            dispatch('setCurrentClip')
        }
    }
}
