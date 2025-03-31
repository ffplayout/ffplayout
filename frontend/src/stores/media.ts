import { defineStore } from 'pinia'

import { useAuth } from './auth'
import { useIndex } from './index'
import { useConfig } from './config'
import { i18n } from '@/i18n'

import { playlistOperations } from '../composables/helper'

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
            const channel = configStore.channels[configStore.i].id
            const crumbs: Crumb[] = []
            let root = '/'

            await fetch(`/api/file/${channel}/browse/`, {
                method: 'POST',
                headers: { ...configStore.contentType, ...authStore.authHeader },
                body: JSON.stringify({ source: path, folders_only: foldersOnly }),
            })
                .then((response) => {
                    if (response.status === 200) {
                        return response.json()
                    } else {
                        indexStore.msgAlert('error', i18n.t('media.notExists'), 3)

                        return {
                            source: '',
                            parent: '',
                            folders: [],
                            files: [],
                        }
                    }
                })
                .then((data) => {
                    const pathStr = `${data.parent}/` + data.source
                    const pathArr = pathStr.split('/')

                    if (path && path !== '/') {
                        for (const crumb of pathArr) {
                            if (crumb === data.parent) {
                                crumbs.push({ text: crumb, path: root })
                            } else if (crumb) {
                                root += crumb + '/'
                                crumbs.push({ text: crumb, path: root })
                            }
                        }
                    } else {
                        crumbs.push({ text: data.parent, path: '' })
                    }

                    if (foldersOnly) {
                        this.folderCrumbs = crumbs
                        data.parent_folders = data.parent_folders?.map((i: any) => ({ uid: genUID(), name: i })) ?? []
                        data.folders = data.folders.map((i: any) => ({ uid: genUID(), name: i }))
                        this.folderList = data
                    } else {
                        this.currentPath = path
                        this.crumbs = crumbs
                        data.parent_folders = data.parent_folders?.map((i: any) => ({ uid: genUID(), name: i })) ?? []
                        data.folders = data.folders.map((i: any) => ({ uid: genUID(), name: i }))
                        this.folderTree = data
                    }
                })

            this.isLoading = false
        },
    },
})
