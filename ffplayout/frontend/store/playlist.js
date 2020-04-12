export const state = () => ({
    playlist: null
})

export const mutations = {
    UPDATE_PLAYLIST (state, list) {
        state.playlist = list
    }
}

export const actions = {
    async getPlaylist ({ commit, dispatch, state, rootState }, { dayStart, date }) {
        console.log(date)
        const response = await this.$axios.get(`api/playlist/?date=${date}`, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        console.log(dayStart)

        const [h, m, s] = dayStart.split(':')
        let begin = parseFloat(h) * 3600 + parseFloat(m) * 60 + parseFloat(s)
        console.log(begin)

        if (response.data) {
            for (const item of response.data.program) {
                item.begin = begin

                begin += (item.out - item.in)
            }

            console.log(response.data)

            commit('UPDATE_PLAYLIST', response.data)
        }
    }
}
