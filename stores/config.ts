import _ from 'lodash'
import { defineStore } from 'pinia'

export const useConfig = defineStore('config', {
    state: () => ({
        configID: 0,
        configCount: 0,
        contentType: { 'content-type': 'application/json;charset=UTF-8' },
        configGui: [] as GuiConfig[],
        configGuiRaw: [] as GuiConfig[],
        playlistLength: 86400.0,
        playout: {} as any,
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
                await authStore.obtainUuid()
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
                .then((response) => {
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

                    indexStore.msgAlert('error', e, 3)
                })
        },

        async setGuiConfig(obj: GuiConfig): Promise<any> {
            const authStore = useAuth()
            const stringObj = _.cloneDeep(obj)
            let response

            if (this.configGuiRaw.some((e) => e.id === stringObj.id)) {
                response = await fetch(`/api/channel/${obj.id}`, {
                    method: 'PATCH',
                    headers: { ...this.contentType, ...authStore.authHeader },
                    body: JSON.stringify(stringObj),
                })
            } else {
                response = await fetch('/api/channel/', {
                    method: 'POST',
                    headers: { ...this.contentType, ...authStore.authHeader },
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
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const indexStore = useIndex()
            const channel = this.configGui[this.configID].id

            await fetch(`/api/playout/config/${channel}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    data.playlist.startInSec = timeToSeconds(data.playlist.day_start ?? 0)
                    data.playlist.lengthInSec = timeToSeconds(data.playlist.length ?? this.playlistLength)

                    if (data.storage.extensions) {
                        data.storage.extensions = data.storage.extensions.join(',')
                    }

                    this.playout = data
                })
                .catch(() => {
                    indexStore.msgAlert('error', 'No playout config found!', 3)
                })
        },

        async setPlayoutConfig(obj: any) {
            const { timeToSeconds } = stringFormatter()
            const authStore = useAuth()
            const channel = this.configGui[this.configID].id

            this.playlistLength = timeToSeconds(obj.playlist.length)
            this.playout.playlist.startInSec = timeToSeconds(obj.playlist.day_start)
            this.playout.playlist.lengthInSec = timeToSeconds(obj.playlist.length)

            if (typeof obj.storage.extensions === 'string') {
                obj.storage.extensions = obj.storage.extensions.replace(' ', '').split(/,|;/)
            }

            const update = await fetch(`/api/playout/config/${channel}`, {
                method: 'PUT',
                headers: { ...this.contentType, ...authStore.authHeader },
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
    },
})
