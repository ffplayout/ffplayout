import { cloneDeep } from 'lodash-es'
import { defineStore } from 'pinia'
import { useRouter } from 'vue-router'

import { useAuth } from './auth'
import { useIndex } from './index'
import { i18n } from '../i18n'
import type { AdvancedConfig } from '../types/advanced_config'
import { stringFormatter } from '../composables/helper'

export const useConfig = defineStore('config', {
    state: () => ({
        i: 0,
        configCount: 0,
        contentType: { 'content-type': 'application/json;charset=UTF-8' },
        channels: [] as Channel[],
        channelsRaw: [] as Channel[],
        playlistLength: 86400.0,
        advanced: {} as AdvancedConfig,
        playout: {} as PlayoutConfigExt,
        outputs: [] as PlayoutOutput[],
        currentUser: 0,
        configUser: {} as User,
        timezone: 'UTC',
        onetimeInfo: true,
        showPlayer: true,
        showRestartModal: false,
    }),

    getters: {},
    actions: {
        async configInit() {
            const authStore = useAuth()
            await authStore.inspectToken()

            if (authStore.isLogin) {
                await authStore.obtainUuid()
                await this.getChannelConfig().then(async () => {
                    await this.getPlayoutConfig()
                    await this.getPlayoutOutputs()
                    await this.getUserConfig()

                    if (authStore.role === 'global_admin') {
                        await this.getAdvancedConfig()
                    }
                })
            }
        },

        logout() {
            const authStore = useAuth()
            const router = useRouter()
            localStorage.removeItem('token')
            localStorage.removeItem('refresh')
            authStore.isLogin = false

            router.push('/')
        },

        async getChannelConfig() {
            const authStore = useAuth()
            const indexStore = useIndex()

            let statusCode = 0
            await fetch('/api/channels', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => {
                    statusCode = response.status

                    return response
                })
                .then((response) => response.json())
                .then((objs) => {
                    if (!objs[0]) {
                        this.logout()
                        throw new Error('User not found')
                    }

                    this.timezone = objs[0].timezone
                    this.channels = objs
                    this.channelsRaw = cloneDeep(objs)
                    this.configCount = objs.length
                })
                .catch((e) => {
                    if (statusCode === 401) {
                        this.logout()
                    }

                    this.channels = [
                        {
                            id: 1,
                            extra_extensions: '',
                            name: 'Channel 1',
                            preview_url: '',
                            public: '',
                            playlists: '',
                            storage: '',
                        },
                    ]

                    indexStore.msgAlert('error', e, 3)
                })
        },

        async getPlayoutConfig() {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const indexStore = useIndex()
            const channel = this.channels[this.i].id

            await fetch(`/api/playout/config/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((resp) => resp.json())
                .then((data: PlayoutConfigExt) => {
                    data.playlist.startInSec = timeToSeconds(data.playlist.day_start ?? 0)
                    data.playlist.lengthInSec = timeToSeconds(data.playlist.length ?? this.playlistLength)

                    this.playout = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async getPlayoutOutputs() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const channel = this.channels[this.i].id

            await fetch(`/api/playout/outputs/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((resp) => resp.json())
                .then((data: PlayoutOutput[]) => {

                    this.outputs = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async getAdvancedConfig() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const channel = this.channels[this.i].id

            await fetch(`/api/playout/advanced/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((resp) => resp.json())
                .then((data: AdvancedConfig) => {
                    this.advanced = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noAdvancedConfig'), 3)
                })
        },

        async setPlayoutConfig(obj: any) {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const channel = this.channels[this.i].id

            this.playlistLength = timeToSeconds(obj.playlist.length)
            this.playout.playlist.startInSec = timeToSeconds(obj.playlist.day_start)
            this.playout.playlist.lengthInSec = timeToSeconds(obj.playlist.length)

            const update = await fetch(`/api/playout/config/${channel}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async setAdvancedConfig() {
            const authStore = useAuth()
            const channel = this.channels[this.i].id

            if (this.advanced?.id > 0) {
                const update = await fetch(`/api/playout/advanced/${channel}`, {
                    method: 'PUT',
                    headers: { ...this.contentType, ...authStore.authHeader },
                    body: JSON.stringify(this.advanced),
                })

                return update
            } else {
                const update = await fetch(`/api/playout/advanced/${channel}/`, {
                    method: 'POST',
                    headers: { ...this.contentType, ...authStore.authHeader },
                    body: JSON.stringify(this.advanced),
                })

                return update
            }
        },

        async getUserConfig() {
            const authStore = useAuth()

            await fetch('/api/user', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    if (data.id === 0) {
                        this.logout()
                        throw new Error('User not found')
                    }

                    this.currentUser = data.id
                    this.configUser = data
                })
        },

        async setUserConfig(obj: any) {
            const authStore = useAuth()

            const update = await fetch(`/api/user/${obj.id}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async addNewUser(user: User) {
            const authStore = useAuth()
            delete user.confirm

            const update = await fetch('/api/user/', {
                method: 'Post',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(user),
            })

            return update
        },

        async restart(res: boolean) {
            if (res) {
                const authStore = useAuth()
                const indexStore = useIndex()
                const channel = this.channels[this.i].id

                await fetch(`/api/control/${channel}/process/`, {
                    method: 'POST',
                    headers: { ...this.contentType, ...authStore.authHeader },
                    body: JSON.stringify({ command: 'restart' }),
                }).catch((e) => {
                    indexStore.msgAlert('error', e.data, 3)
                })
            }

            this.showRestartModal = false
        },
    },
})
