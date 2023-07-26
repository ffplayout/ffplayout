import _ from 'lodash'
import { defineStore } from 'pinia'

const { timeToSeconds } = stringFormatter()

import { useAuth } from '~/stores/auth'
import { useIndex } from '~/stores/index'

interface GuiConfig {
    id: number
    config_path: string
    extra_extensions: string | string[]
    name: string
    preview_url: string
    service: string
    uts_offset?: number
}

interface User {
    username: string
    mail: string
    password?: string
}

export const useConfig = defineStore('config', {
    state: () => ({
        configID: 0,
        configCount: 0,
        configGui: [] as GuiConfig[],
        configGuiRaw: [] as GuiConfig[],
        startInSec: 0,
        playlistLength: 86400.0,
        configPlayout: {} as any,
        currentUser: '',
        configUser: {} as User,
        utcOffset: 0,
    }),

    getters: {},
    actions: {
        async nuxtClientInit() {
            const authStore = useAuth()

            authStore.inspectToken()

            if (authStore.isLogin) {
                await this.getGuiConfig()
                await this.getPlayoutConfig()
                await this.getUserConfig()
            }
        },

        async getGuiConfig() {
            const authStore = useAuth()
            const indexStore = useIndex()

            let statusCode = 0
            await fetch('/api/channels', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then(response => {
                    statusCode = response.status

                    return response
                })
                .then((response) => response.json())
                .then((objs) => {
                    this.utcOffset = objs[0].utc_offset
                    this.configGui = objs
                    this.configGuiRaw = _.cloneDeep(objs)
                    this.configCount = objs.length
                })
                .catch((e) => {
                    if (statusCode === 401) {
                        const cookie = useCookie('token')
                        cookie.value = null
                        authStore.isLogin = false

                        navigateTo('/')
                    }

                    this.configGui = [
                        {
                            id: 1,
                            config_path: '',
                            extra_extensions: '',
                            name: 'Channel 1',
                            preview_url: '',
                            service: '',
                            uts_offset: 0,
                        },
                    ]

                    indexStore.alertMsg = e
                    indexStore.alertVariant = 'alert-danger'
                    indexStore.showAlert = true
                })
        },

        async setGuiConfig(obj: GuiConfig): Promise<any> {
            const authStore = useAuth()
            const stringObj = _.cloneDeep(obj)
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }
            let response

            if (this.configGuiRaw.some((e) => e.id === stringObj.id)) {
                response = await fetch(`/api/channel/${obj.id}`, {
                    method: 'PATCH',
                    headers: { ...contentType, ...authStore.authHeader },
                    body: JSON.stringify(stringObj),
                })
            } else {
                response = await fetch('/api/channel/', {
                    method: 'POST',
                    headers: { ...contentType, ...authStore.authHeader },
                    body: JSON.stringify(stringObj),
                })

                const json = await response.json()
                const guiConfigs = []

                for (const obj of this.configGui) {
                    if (obj.name === stringObj.name) {
                        guiConfigs.push(json)
                    } else {
                        guiConfigs.push(obj)
                    }
                }

                this.configGui = guiConfigs
                this.configGuiRaw = _.cloneDeep(guiConfigs)
                this.configCount = guiConfigs.length
            }

            await this.getPlayoutConfig()

            return response
        },

        async getPlayoutConfig() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const channel = this.configGui[this.configID].id

            await fetch(`/api/playout/config/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    if (data.playlist.day_start) {
                        this.startInSec = timeToSeconds(data.playlist.day_start)
                    }

                    if (data.playlist.length) {
                        this.playlistLength = timeToSeconds(data.playlist.length)
                    }

                    if (data.storage.extensions) {
                        data.storage.extensions = data.storage.extensions.join(',')
                    }

                    this.configPlayout = data
                })
                .catch(() => {
                    indexStore.alertMsg = 'No playout config found!'
                    indexStore.alertVariant = 'alert-danger'
                    indexStore.showAlert = true
                })
        },

        async setPlayoutConfig(obj: any) {
            const authStore = useAuth()
            const channel = this.configGui[this.configID].id
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }

            this.startInSec = timeToSeconds(obj.playlist.day_start)
            this.playlistLength = timeToSeconds(obj.playlist.length)

            if (typeof obj.storage.extensions === 'string') {
                obj.storage.extensions = obj.storage.extensions.replace(' ', '').split(/,|;/)
            }

            const update = await fetch(`/api/playout/config/${channel}`, {
                method: 'PUT',
                headers: { ...contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async getUserConfig() {
            const authStore = useAuth()

            await fetch('/api/user', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    this.currentUser = data.username
                    this.configUser = data
                })
        },

        async setUserConfig(obj: any) {
            const authStore = useAuth()
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }

            const update = await fetch(`/api/user/${obj.id}`, {
                method: 'PUT',
                headers: { ...contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },
    },
})
