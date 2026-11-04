import { cloneDeep } from 'es-toolkit/object'
import { defineStore } from 'pinia'
import { useRouter } from 'vue-router'

import { authFetch } from '@/composables/authFetch'
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
            desktop_available: false,
            moq: { video: [], audio: [] },
            hls: { video: [], audio: [] },
            rtmp: { video: [], audio: [] },
            srt: { video: [], audio: [] },
            udp: { video: [], audio: [] },
            custom: { video: [], audio: [] },
            recording: { video: [], audio: [] },
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
        async configInit(channelId?: number) {
            const authStore = useAuth()

            if (authStore.isLogin) {
                await authStore.obtainUuid()
                await this.getChannelConfig().then(async () => {
                    this.selectChannel(channelId)
                    await this.getPlayoutConfig()
                    await this.getPlayoutOutputs()
                    await this.getPlayoutCodecs()
                    await this.getUserConfig()
                })
            }
        },

        async logout() {
            const authStore = useAuth()
            const router = useRouter()
            await authStore.logout()

            await router.push({ name: 'login' })
        },

        async getChannelConfig() {
            const authStore = useAuth()
            const indexStore = useIndex()

            await authFetch<Channel[]>('/api/channels', {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((objs) => {
                    if (!objs[0]) {
                        this.logout()
                        throw new Error('User not found')
                    }

                    this.timezone = objs[0].timezone ?? 'UTC'
                    this.channels = objs
                    this.channelsRaw = cloneDeep(objs)
                    this.configCount = objs.length
                })
                .catch((e) => {
                    if (e.response?.status === 401) {
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

        selectChannel(channelId?: number) {
            const index = this.channels.findIndex((channel) => channel.id === channelId)
            this.i = index >= 0 ? index : 0
        },

        async getPlayoutConfig() {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const indexStore = useIndex()
            const id = this.channels[this.i]?.id

            await authFetch<PlayoutConfigExt>(`/api/playout/config/${id}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((data: PlayoutConfigExt) => {
                    data.playlist.startInSec = timeToSeconds(data.playlist.day_start ?? 0)
                    data.playlist.lengthInSec = timeToSeconds(data.playlist.length ?? this.playlistLength)

                    data.outputs ??= []
                    for (const output of [data.output, ...data.outputs]) {
                        output.public_moq ??= false
                        output.enabled ??= true
                        output.label ??= ''
                        output.inherit_resolution ??= false
                        output.share_encoding ??= true
                        output.encoding_source_id ??= null
                        output.encoding_variant ??= ''
                    }
                    const previousProgram =
                        [data.output, ...data.outputs].find((output) => output.enabled && output.mode === 'desktop') ??
                        data.output
                    data.program ??= {
                        width: previousProgram.width,
                        height: previousProgram.height,
                        fps: previousProgram.fps,
                    }
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

            await authFetch<PlayoutOutput[]>(`/api/playout/outputs/${id}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((data: PlayoutOutput[]) => {
                    if (this.channels[this.i]?.id === id) this.outputs = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async getPlayoutCodecs() {
            const authStore = useAuth()
            const indexStore = useIndex()
            const id = this.channels[this.i]?.id

            await authFetch<PlayoutCodecOptions>(`/api/playout/codecs/${id}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((data: PlayoutCodecOptions) => {
                    this.outputCodecs = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', i18n.t('config.noPlayoutConfig'), 3)
                })
        },

        async setPlayoutConfig(obj: any, scope?: 'ingest') {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const id = this.channels[this.i]?.id

            if (scope === 'ingest') {
                return authFetch<{ requires_restart: boolean }>(`/api/playout/config/${id}?scope=ingest`, {
                    method: 'PUT',
                    headers: { ...this.contentType, ...authStore.authHeader },
                    body: JSON.stringify(obj),
                })
            }

            this.playlistLength = timeToSeconds(obj.playlist.length)
            this.playout.playlist.startInSec = timeToSeconds(obj.playlist.day_start)
            this.playout.playlist.lengthInSec = timeToSeconds(obj.playlist.length)

            const collection = [obj.output, ...(obj.outputs ?? [])] as PlayoutConfigExt['output'][]
            const firstEnabled = collection.findIndex((output) => output.enabled)
            // Preserve the legacy wire format while exposing a collection in the UI.
            if (firstEnabled > 0) collection.unshift(collection.splice(firstEnabled, 1)[0]!)
            obj.output = collection[0]
            obj.outputs = collection.slice(1)
            for (const output of collection) {
                output.share_encoding = output.encoding_source_id !== null
                output.fps = obj.program?.fps ?? obj.output.fps
                if (output.inherit_resolution && obj.program) {
                    output.width = obj.program.width
                    output.height = obj.program.height
                }
            }

            const update = await authFetch<{ requires_restart: boolean }>(`/api/playout/config/${id}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async applyAudioEffects(volume: number) {
            const authStore = useAuth()
            const id = this.channels[this.i]?.id

            return authFetch<void>(`/api/control/${id}/audio`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify({ volume }),
            })
        },

        async getUserConfig() {
            const authStore = useAuth()

            await authFetch<User>('/api/user', {
                method: 'GET',
                headers: authStore.authHeader,
            }).then((data) => {
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

            const update = await authFetch<User>(`/api/user/${obj.id}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
                body: JSON.stringify(obj),
            })

            return update
        },

        async addNewUser(user: User) {
            const authStore = useAuth()
            delete user.confirm

            const update = await authFetch<User>('/api/user', {
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

                await authFetch(`/api/control/${id}/process`, {
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
