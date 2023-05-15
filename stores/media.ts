import { defineStore } from 'pinia'

import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'

export const useMedia = defineStore('media', {
    state: () => ({
        currentPath: '',
        crumbs: [] as Crumb[],
        folderTree: {} as FileFolderObject,
        folderList: {} as FolderObject,
        folderCrumbs: [] as Crumb[],
    }),

    getters: {},
    actions: {
        async getTree(path: string, foldersOnly: boolean = false) {
            const authStore = useAuth()
            const configStore = useConfig()
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }
            const channel = configStore.configGui[configStore.configID].id
            const crumbs: Crumb[] = []
            let root = '/'

            await fetch(`/api/file/${channel}/browse/`, {
                method: 'POST',
                headers: { ...contentType, ...authStore.authHeader },
                body: JSON.stringify({ source: path, folders_only: foldersOnly }),
            })
                .then((response) => response.json())
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
                        this.folderList = data
                    } else {
                        this.currentPath = path
                        this.crumbs = crumbs
                        this.folderTree = data
                    }
                })
        }
    },
})
