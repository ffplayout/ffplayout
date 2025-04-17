import dayjs from 'dayjs'
import { differenceWith, isEqual, omit } from 'lodash-es'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

import { defineStore } from 'pinia'

import { i18n } from '@/i18n'
import { playlistOperations } from '@/composables/helper'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

dayjs.extend(utc)
dayjs.extend(timezone)

const { processPlaylist } = playlistOperations()

export const usePlaylist = defineStore('playlist', {
    state: () => ({
        playlist: [] as PlaylistItem[],
        isLoading: true,
        listDate: dayjs().format('YYYY-MM-DD'),
        progressValue: 0,
        current: {} as PlaylistItem,
        currentIndex: 0,
        ingestRuns: false,
        elapsedSec: 0,
        shift: 0,
        playoutIsRunning: false,
        last_channel: 0,
        firstLoad: true,
        scrollToItem: false,
    }),

    getters: {},
    actions: {
        async getPlaylist(date: string) {
            const authStore = useAuth()
            const configStore = useConfig()
            const indexStore = useIndex()
            const channel = configStore.channels[configStore.i].id

            await fetch(`/api/playlist/${channel}?date=${date}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then(async (response) => {
                    if (!response.ok) {
                        throw new Error(await response.text())
                    }

                    return response.json()
                })
                .then((data: Playlist) => {
                    if (data.program) {
                        const programData = processPlaylist(date, data.program, false)

                        if (
                            channel === this.last_channel &&
                            this.playlist.length > 0 &&
                            programData.length > 0 &&
                            (this.playlist[0].date === date || configStore.playout.playlist.infinit) &&
                            differenceWith(this.playlist, programData, (a, b) => {
                                return isEqual(omit(a, ['uid']), omit(b, ['uid']))
                            }).length > 0
                        ) {
                            indexStore.msgAlert('warning', i18n.t('player.unsavedProgram'), 3)
                        } else {
                            this.playlist = programData ?? []
                        }
                    }
                })
                .catch((e) => {
                    if (e.status >= 400) {
                        indexStore.msgAlert('error', e.data, 5)
                    } else if (
                        channel === this.last_channel &&
                        this.playlist.length > 0 &&
                        this.playlist[0].date === date
                    ) {
                        indexStore.msgAlert('warning', i18n.t('player.unsavedProgram'), 3)
                    } else {
                        this.playlist = []
                    }
                })

            this.last_channel = channel
        },

        setStatus(item: PlayoutStatus) {
            this.playoutIsRunning = true
            this.current = item.media
            this.currentIndex = item.index
            this.elapsedSec = item.elapsed
            this.ingestRuns = item.ingest
            this.shift = item.shift

            this.progressValue = (this.elapsedSec * 100) / this.current.out
        },
    },
})
