// convert time (00:00:00) string to seconds
function timeToSeconds (time) {
    const t = time.split(':')
    return parseInt(t[0]) * 3600 + parseInt(t[1]) * 60 + parseInt(t[2])
}

export const state = () => ({
    playlist: null,
    clockStart: true,
    progressValue: 0,
    currentClip: null,
    timeStr: null,
    timeLeft: null
})

export const mutations = {
    UPDATE_PLAYLIST (state, list) {
        state.playlist = list
    },
    SET_CLOCK_START (state, bol) {
        state.clockStart = bol
    },
    SET_PROGRESS_VALUE (state, value) {
        state.clockStart = value
    },
    SET_CURRENT_CLIP (state, clip) {
        state.currentClip = clip
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
        const response = await this.$axios.get(`api/playlist/?date=${date}`, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        const [h, m, s] = dayStart.split(':')
        let begin = parseFloat(h) * 3600 + parseFloat(m) * 60 + parseFloat(s)

        if (response.data && response.data.program) {
            for (const item of response.data.program) {
                item.begin = begin

                begin += (item.out - item.in)
            }

            commit('UPDATE_PLAYLIST', response.data.program)
            dispatch('animClock')
        }
    },

    async animClock ({ commit, dispatch, state, rootState }) {
        // sleep timer
        function sleep (ms) {
            return new Promise(resolve => setTimeout(resolve, ms))
        }

        let start = timeToSeconds(rootState.config.configPlayout.playlist.day_start)
        let time

        if (state.clockStart) {
            commit('SET_CLOCK_START', false)

            // loop over clips in program list from today
            for (let i = 0; i < state.playlist.length; i++) {
                let breakOut = false
                const duration = state.playlist[i].out - state.playlist[i].in
                let playTime = timeToSeconds(this.$dayjs().format('HH:mm:ss')) - start
                let updateSrc = true

                // animate the progress bar
                while (playTime <= duration) {
                    if (updateSrc) {
                        commit('SET_CURRENT_CLIP', state.playlist[i].source)
                        console.log(state.currentClip)
                        updateSrc = false
                    }
                    await sleep(1000)
                    const pValue = playTime * 100 / duration
                    if (pValue < state.progressValue) {
                        breakOut = true
                        break
                    }

                    time = this.$dayjs().format('HH:mm:ss')
                    commit('SET_PROGRESS_VALUE', pValue)
                    commit('SET_TIME', time)
                    playTime = timeToSeconds(time) - start
                    commit('SET_TIME_LEFT', new Date((duration - playTime) * 1000).toISOString().substr(11, 8))
                }

                start += duration

                if (breakOut) {
                    break
                }

                // reset progress
                commit('SET_PROGRESS_VALUE', 0)
            }
        }
    }

}
