import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

import { defineStore } from 'pinia'

dayjs.extend(utc)
dayjs.extend(timezone)

import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'

const authStore = useAuth()
const configStore = useConfig()
const { timeToSeconds } = stringFormatter()
const { processPlaylist } = playlistOperations()

export const usePlaylist = defineStore('playlist', {
    state: () => ({
        playlist: [] as PlaylistItem[],
        progressValue: 0,
        currentClip: 'No clip is playing',
        currentClipIndex: -1,
        currentClipStart: 0,
        currentClipDuration: 0,
        currentClipIn: 0,
        currentClipOut: 0,
        remainingSec: 0,
        playoutIsRunning: true,
    }),

    getters: {},
    actions: {
        updatePlaylist(list: any) {
            this.playlist = list
        },

        async getPlaylist(date: string) {
            const timeInSec = timeToSeconds(dayjs().utcOffset(configStore.utcOffset).format('HH:mm:ss'))
            const channel = configStore.configGui[configStore.configID].id
            let dateToday = dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD')

            if (configStore.startInSec > timeInSec) {
                dateToday = dayjs(dateToday).utcOffset(configStore.utcOffset).subtract(1, 'day').format('YYYY-MM-DD')
            }

            await fetch(`/api/playlist/${channel}?date=${date}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => response.json())
                .then((data) => {
                    if (data.program) {
                        this.updatePlaylist(
                            processPlaylist(configStore.startInSec, configStore.playlistLength, data.program, false)
                        )
                    }
                })
                .catch(() => {
                    this.updatePlaylist([])
                })
        },

        async playoutStat() {
            const channel = configStore.configGui[configStore.configID].id

            await fetch(`/api/control/${channel}/media/current`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => {
                    if (response.status === 503) {
                        this.playoutIsRunning = false
                    }

                    return response.json()
                })
                .then((data) => {
                    if (data.result && data.result.played_sec) {
                        this.playoutIsRunning = true
                        const obj = data.result
                        this.currentClip = obj.current_media.source
                        this.currentClipIndex = obj.index
                        this.currentClipStart = obj.start_sec
                        this.currentClipDuration = obj.current_media.duration
                        this.currentClipIn = obj.current_media.seek
                        this.currentClipOut = obj.current_media.out
                    }
                })
                .catch(() => {
                    this.playoutIsRunning = false
                })
        },
    },
})
