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
    async getTree ({ commit, dispatch, state, rootState }, { extensions, path }) {
        const crumbs = []
        let root = '/'
        const response = await this.$axios.get(`api/media/?extensions=${extensions}&path=${path}`, { headers: { Authorization: 'Bearer ' + rootState.auth.jwtToken } })

        if (response.data.tree) {
            const pathArr = response.data.tree[0].split('/')

            if (response.data.tree[1].length === 0) {
                response.data.tree[1].push(pathArr[pathArr.length - 1])
            }

            if (path) {
                for (const crumb of pathArr) {
                    if (crumb) {
                        root += crumb + '/'
                        crumbs.push({ text: crumb, path: root })
                    }
                }
            } else {
                crumbs.push({ text: pathArr[pathArr.length - 1], path: '' })
            }

            // console.log(crumbs)
            commit('UPDATE_CURRENT_PATH', path)
            commit('UPDATE_CRUMBS', crumbs)
            commit('UPDATE_FOLDER_TREE', response.data)
        }
    }
}
