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
            // post_ffmpeg_param is normally a object, for the form we convert it to a string
            response.data.out.post_ffmpeg_param = JSON.stringify(response.data.out.post_ffmpeg_param).replace('{', '').replace('}', '').replace(/":/g, ' ').replace(/","/g, ' ').replace(/"/g, '')
            commit('UPDATE_CONFIG', response.data)
        }
    },

    async setConfig ({ commit, state, rootState }, obj) {
        const ffmpegParam = new Map()
        const newObj = JSON.parse(JSON.stringify(obj))
        const params = obj.out.post_ffmpeg_param.split(' ')

        for (let i = 0; i < params.length; i++) {
            if (i % 2) {
                continue
            } else {
                ffmpegParam.set(params[i], (params[i + 1]) ? params[i + 1] : null)
            }
        }

        newObj.out.post_ffmpeg_param = Object.fromEntries(ffmpegParam)
        const response = await this.$axios.post('api/config/', { body: { name: 'andi', des: 'welcome' } })
        console.log(response)
    }
}
