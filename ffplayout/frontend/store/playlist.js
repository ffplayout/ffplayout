// convert time (00:00:00) string to seconds
function timeToSeconds (time) {
    const t = time.split(':')
    return parseInt(t[0]) * 3600 + parseInt(t[1]) * 60 + parseInt(t[2])
}

function secToHMS (sec) {
    let hours = Math.floor(sec / 3600)
    sec %= 3600
    let minutes = Math.floor(sec / 60)
    let seconds = sec % 60

    minutes = String(minutes).padStart(2, '0')
    hours = String(hours).padStart(2, '0')
    seconds = String(parseInt(seconds)).padStart(2, '0')
    return hours + ':' + minutes + ':' + seconds
}

// sleep timer
function sleep (ms) {
    return new Promise(resolve => setTimeout(resolve, ms))
}

/*
const counte = (function main (counter) {
    console.log(counter)

    if (counter < 20) {
        setTimeout(main, 1000, counter + 1)
    }
})(0)
*/
export const state = () => ({
    playlist: null,
    clockStart: true,
    progressValue: 0,
    currentClip: 'No clips is playing',
    timeStr: '00:00:00',
    timeLeft: '00:00:00'
})

export const mutations = {
    UPDATE_PLAYLIST (state, list) {
        state.playlist = list
    },
    SET_CLOCK_START (state, bol) {
        state.clockStart = bol
    },
    SET_PROGRESS_VALUE (state, value) {
        state.progressValue = value
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

            if (process.browser) {
                // TODO: find a better way for non-blocking animation
                // dispatch('animClock')
            }
        }
    },

    async animClock ({ commit, dispatch, state, rootState }) {
        let start = timeToSeconds(rootState.config.configPlayout.playlist.day_start)
        let time

        if (state.clockStart) {
            commit('SET_CLOCK_START', false)

            // loop over clips in program list from today
            for (let i = 0; i < state.playlist.length; i++) {
                const duration = state.playlist[i].out - state.playlist[i].in
                let playTime = timeToSeconds(this.$dayjs().format('HH:mm:ss')) - start
                let updateSrc = true

                // animate the progress bar
                while (playTime <= duration) {
                    if (updateSrc) {
                        commit('SET_CURRENT_CLIP', state.playlist[i].source)
                        updateSrc = false
                    }

                    const pValue = playTime * 100 / duration

                    time = this.$dayjs().format('HH:mm:ss')
                    commit('SET_PROGRESS_VALUE', pValue)
                    commit('SET_TIME', time)
                    playTime += 1
                    commit('SET_TIME_LEFT', secToHMS(duration - playTime))
                    await sleep(1000)
                }

                start += duration

                // reset progress
                commit('SET_PROGRESS_VALUE', 0)
            }
        }
    }

}
