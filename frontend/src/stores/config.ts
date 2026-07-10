import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'
import { defineStore } from 'pinia'
import { useRouter } from 'vue-router'

import { useAuth } from './auth'
import { useIndex } from './index'
import { i18n } from '../i18n'
import { stringFormatter } from '../composables/helper'

export const useConfig = defineStore('config', {
    state: () => ({
        i: 0,
        configCount: 0,
        contentType: { 'content-type': 'application/json;charset=UTF-8' },
        channels: [] as Channel[],
        channelsRaw: [] as Channel[],
        playlistLength: 86400.0,
        playout: {} as PlayoutConfigExt,
        playoutSaved: {} as PlayoutConfigExt,
        outputs: [] as PlayoutOutput[],
        outputCodecs: {
            hls: { video: [], audio: [] },
            rtmp: { video: [], audio: [] },
            srt: { video: [], audio: [] },
            udp: { video: [], audio: [] },
        } as PlayoutCodecOptions,
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
                    await this.getPlayoutCodecs()
                    await this.getUserConfig()

                })
            }
        },

        logout() {
            const authStore = useAuth()
            const router = useRouter()
            localStorage.removeItem('token')
            localStorage.removeItem('refresh')
            authStore.isLogin = false

            router.push({ name: 'login' })
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
            const id = this.channels[this.i]?.id

            await fetch(`/api/playout/config/${id}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((resp) => resp.json())
                .then((data: PlayoutConfigExt) => {
                    data.playlist.startInSec = timeToSeconds(data.playlist.day_start ?? 0)
                    data.playlist.lengthInSec = timeToSeconds(data.playlist.length ?? this.playlistLength)

                    this.playout = data
                    this.playoutSaved = cloneDeep(data)
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async getPlayoutOutputs() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const id = this.channels[this.i]?.id

            await fetch(`/api/playout/outputs/${id}`, {
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

        async getPlayoutCodecs() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const id = this.channels[this.i]?.id

            await fetch(`/api/playout/codecs/${id}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((resp) => resp.json())
                .then((data: PlayoutCodecOptions) => {
                    this.outputCodecs = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async setPlayoutConfig(obj: any) {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const id = this.channels[this.i]?.id

            this.playlistLength = timeToSeconds(obj.playlist.length)
            this.playout.playlist.startInSec = timeToSeconds(obj.playlist.day_start)
            this.playout.playlist.lengthInSec = timeToSeconds(obj.playlist.length)

            const update = await fetch(`/api/playout/config/${id}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        playoutChangeSummary() {
            if (!this.playoutSaved.processing) {
                return { requiresRestart: true, volumeChanged: false }
            }

            const current = cloneDeep(this.playout)
            const saved = cloneDeep(this.playoutSaved)
            const volumeChanged = current.processing.volume !== saved.processing.volume
            current.processing.volume = saved.processing.volume

            return {
                requiresRestart: !isEqual(current, saved),
                volumeChanged,
            }
        },

        async applyAudioEffects(volume: number) {
            const authStore = useAuth()
            const id = this.channels[this.i]?.id

            return fetch(`/api/control/${id}/audio`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify({ volume }),
            })
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

            const update = await fetch('/api/user', {
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
                const id = this.channels[this.i]?.id

                await fetch(`/api/control/${id}/process`, {
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
