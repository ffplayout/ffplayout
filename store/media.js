export const state = () => ({
    currentPath: null,
    crumbs: [],
    folderTree: {}
})

export const mutations = {
    UPDATE_CURRENT_PATH (state, path) {
        state.currentPath = path
    },
    UPDATE_CRUMBS (state, crumbs) {
        state.crumbs = crumbs
    },
    UPDATE_FOLDER_TREE (state, tree) {
        state.folderTree = tree
    }
}

export const actions = {
    async getTree ({ commit, dispatch, state, rootState }, { path }) {
        const crumbs = []
        let root = '/'
        const channel = rootState.config.configGui[rootState.config.configID].id
        const response = await this.$axios.post(
            `api/file/${channel}/browse/`, { source: path })

        if (response.data) {
            const pathStr = 'Home/' + response.data.source
            const pathArr = pathStr.split('/')

            if (path) {
                for (const crumb of pathArr) {
                    if (crumb === 'Home') {
                        crumbs.push({ text: crumb, path: root })
                    } else if (crumb) {
                        root += crumb + '/'
                        crumbs.push({ text: crumb, path: root })
                    }
                }
            } else {
                crumbs.push({ text: 'Home', path: '' })
            }

            commit('UPDATE_CURRENT_PATH', path)
            commit('UPDATE_CRUMBS', crumbs)
            commit('UPDATE_FOLDER_TREE', response.data)
        }
    }
}
