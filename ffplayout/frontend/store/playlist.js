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

export const state = () => ({
    playlist: null,
    playlistToday: [],
    playlistChannel: 'Channel 1',
    progressValue: 0,
    currentClip: 'No clips is playing',
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
    UPDATE_PLAYLIST_CHANNEL (state, channel) {
        state.playlistChannel = channel
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
    async getPlaylist ({ commit, dispatch, state }, { dayStart, date }) {
        const response = await this.$axios.get(`api/player/playlist/?date=${date}`)

        if (response.data && response.data.program) {
            commit('UPDATE_PLAYLIST_CHANNEL', response.data.channel)
            commit('UPDATE_PLAYLIST', this.$processPlaylist(dayStart, response.data.program))

            if (date === this.$dayjs().format('YYYY-MM-DD')) {
                commit('UPDATE_TODAYS_PLAYLIST', JSON.parse(JSON.stringify(response.data.program)))
            }
        } else {
            commit('UPDATE_PLAYLIST', [])
        }
    },

    animClock ({ commit, dispatch, state }, { dayStart }) {
        let start = this.$timeToSeconds(dayStart)

        // loop over clips in program list from today
        for (let i = 0; i < state.playlistToday.length; i++) {
            const duration = state.playlistToday[i].out - state.playlistToday[i].in
            const time = this.$dayjs().add(1, 'seconds').format('HH:mm:ss')
            const playTime = this.$timeToSeconds(time) - start

            // set current clip and progressbar value
            if (playTime <= duration) {
                commit('SET_CURRENT_CLIP', state.playlistToday[i].source)
                commit('SET_PROGRESS_VALUE', playTime * 100 / duration)
                commit('SET_TIME', time)
                commit('SET_TIME_LEFT', secToHMS(duration - playTime))
                break
            }

            start += duration
        }
    }
}
