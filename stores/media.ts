import { defineStore } from 'pinia'

const { genUID } = playlistOperations()

export const useMedia = defineStore('media', {
    state: () => ({
        currentPath: '',
        crumbs: [] as Crumb[],
        folderTree: {} as FileFolderObject,
        folderList: {} as FolderObject,
        folderCrumbs: [] as Crumb[],
        isLoading: false,
    }),

    getters: {},
    actions: {
        async getTree(path: string, foldersOnly: boolean = false) {
            if (!foldersOnly) {
                this.isLoading = true
            }

            const authStore = useAuth()
            const configStore = useConfig()
            const indexStore = useIndex()
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }
            const channel = configStore.configGui[configStore.configID].id
            const crumbs: Crumb[] = []
            let root = '/'

            await fetch(`/api/file/${channel}/browse/`, {
                method: 'POST',
                headers: { ...contentType, ...authStore.authHeader },
                body: JSON.stringify({ source: path, folders_only: foldersOnly }),
            })
                .then((response) => {
                    if (response.status === 200) {
                        return response.json()
                    } else {
                        indexStore.msgAlert('error', 'Storage not exist!', 3)

                        return {
                            source: '',
                            parent: '',
                            folders: [],
                            files: [],
                        }
                    }
                })
                .then((data) => {
                    const pathStr = 'Home/' + data.source
                    const pathArr = pathStr.split('/')

                    if (path && path !== '/') {
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

                    if (foldersOnly) {
                        this.folderCrumbs = crumbs
                        data.parent_folders = data.parent_folders.map((i: any) => ({ uid: genUID(), name: i }))
                        data.folders = data.folders.map((i: any) => ({ uid: genUID(), name: i }))
                        this.folderList = data
                    } else {
                        this.currentPath = path
                        this.crumbs = crumbs
                        data.parent_folders = data.parent_folders.map((i: any) => ({ uid: genUID(), name: i }))
                        data.folders = data.folders.map((i: any) => ({ uid: genUID(), name: i }))
                        this.folderTree = data
                    }
                })

            this.isLoading = false
        },
    },
})
