import dayjs from 'dayjs'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

import { defineStore } from 'pinia'

dayjs.extend(utc)
dayjs.extend(timezone)

const { processPlaylist } = playlistOperations()

export const usePlaylist = defineStore('playlist', {
    state: () => ({
        playlist: [] as PlaylistItem[],
        isLoading: true,
        listDate: dayjs().format('YYYY-MM-DD'),
        progressValue: 0,
        currentClip: '',
        currentClipIndex: 0,
        currentClipStart: 0,
        currentClipDuration: 0,
        currentClipIn: 0,
        currentClipOut: 0,
        ingestRuns: false,
        remainingSec: 0,
        playoutIsRunning: false,
    }),

    getters: {},
    actions: {
        async getPlaylist(date: string) {
            const { $_, $i18n } = useNuxtApp()
            const authStore = useAuth()
            const configStore = useConfig()
            const indexStore = useIndex()
            const channel = configStore.configGui[configStore.configID].id
            let statusCode = 0

            await fetch(`/api/playlist/${channel}?date=${date}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((response) => {
                    statusCode = response.status

                    return response.json()
                })
                .then((data) => {
                    if (data.program) {
                        const programData = processPlaylist(date, data.program, false)

                        if (
                            this.playlist.length > 0 &&
                            programData.length > 0 &&
                            this.playlist[0].date === date &&
                            $_.differenceWith(this.playlist, programData, (a, b) => {
                                return $_.isEqual($_.omit(a, ['uid']), $_.omit(b, ['uid']))
                            }).length > 0
                        ) {
                            indexStore.msgAlert('warning', $i18n.t('player.unsavedProgram'), 3)
                        } else {
                            this.playlist = programData ?? []
                        }
                    }
                })
                .catch((e) => {
                    if (statusCode >= 400) {
                        indexStore.msgAlert('error', e, 3)
                    } else if (this.playlist.length > 0 && this.playlist[0].date === date) {
                        indexStore.msgAlert('warning', $i18n.t('player.unsavedProgram'), 3)
                    } else {
                        this.playlist = []
                    }
                })
        },

        async playoutStat() {
            const authStore = useAuth()
            const configStore = useConfig()
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
                    if (data && data.played_sec) {
                        this.playoutIsRunning = true
                        this.currentClip = data.current_media.source
                        this.currentClipIndex = data.index
                        this.currentClipStart = data.start_sec
                        this.currentClipDuration = data.current_media.duration
                        this.currentClipIn = data.current_media.seek
                        this.currentClipOut = data.current_media.out
                        this.remainingSec = data.remaining_sec
                        this.ingestRuns = data.ingest_runs
                    }
                })
                .catch(() => {
                    this.playoutIsRunning = false
                })
        },
    },
})
