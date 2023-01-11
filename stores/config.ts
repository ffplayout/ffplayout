import _ from 'lodash'
import { defineStore } from 'pinia'

const { timeToSeconds } = stringFormatter()

import { useAuth } from '~/stores/auth'
import { useIndex } from '~/stores/index'
const authStore = useAuth()
const indexStore = useIndex()

interface GuiConfig {
    id: number
    config_path: string
    extra_extensions: string
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
        updateConfigID(id: number) {
            this.configID = id
        },

        updateConfigCount(count: number) {
            this.configCount = count
        },

        updateGuiConfig(config: GuiConfig[]) {
            this.configGui = config
        },

        updateGuiConfigRaw(config: GuiConfig[]) {
            this.configGuiRaw = config
        },

        updateStartTime(sec: number) {
            this.startInSec = sec
        },

        updatePlaylistLength(sec: number) {
            this.playlistLength = sec
        },

        updatePlayoutConfig(config: any) {
            this.configPlayout = config
        },

        setCurrentUser(user: string) {
            this.currentUser = user
        },

        updateUserConfig(config: User) {
            this.configUser = config
        },

        updateUtcOffset(offset: number) {
            this.utcOffset = offset
        },

        async nuxtClientInit() {
            authStore.inspectToken()

            if (authStore.isLogin) {
                await this.getGuiConfig()
                await this.getPlayoutConfig()
                await this.getUserConfig()
            }
        },

        async getGuiConfig() {
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
                    this.updateUtcOffset(objs[0].utc_offset)
                    this.updateGuiConfig(objs)
                    this.updateGuiConfigRaw(_.cloneDeep(objs))
                    this.updateConfigCount(objs.length)
                })
                .catch((e) => {
                    if (statusCode === 401) {
                        const cookie = useCookie('token')
                        cookie.value = null
                        authStore.isLogin = false

                        navigateTo('/')
                    }

                    this.updateGuiConfig([
                        {
                            id: 1,
                            config_path: '',
                            extra_extensions: '',
                            name: 'Channel 1',
                            preview_url: '',
                            service: '',
                            uts_offset: 0,
                        },
                    ])

                    indexStore.alertMsg = e
                    indexStore.alertVariant = 'alert-danger'
                    indexStore.showAlert = true
                })
        },

        async setGuiConfig(obj: GuiConfig): Promise<any> {
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

                this.updateGuiConfig(guiConfigs)
                this.updateGuiConfigRaw(_.cloneDeep(guiConfigs))
                this.updateConfigCount(guiConfigs.length)

                await this.getPlayoutConfig()
            }

            return response
        },

        async getPlayoutConfig() {
            const channel = this.configGui[this.configID].id

            await fetch(`/api/playout/config/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    if (data.playlist.day_start) {
                        const start = timeToSeconds(data.playlist.day_start)
                        this.updateStartTime(start)
                    }

                    if (data.playlist.length) {
                        const length = timeToSeconds(data.playlist.length)
                        this.updatePlaylistLength(length)
                    }

                    this.updatePlayoutConfig(data)
                })
                .catch(() => {
                    indexStore.alertMsg = 'No playout config found!'
                    indexStore.alertVariant = 'alert-danger'
                    indexStore.showAlert = true
                })
        },

        async setPlayoutConfig(obj: any) {
            const channel = this.configGui[this.configID].id
            const contentType = { 'content-type': 'application/json;charset=UTF-8' }

            const update = await fetch(`/api/playout/config/${channel}`, {
                method: 'PUT',
                headers: { ...contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async getUserConfig() {
            await fetch('/api/user', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    this.setCurrentUser(data.username)
                    this.updateUserConfig(data)
                })
        },

        async setUserConfig(obj: any) {
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
