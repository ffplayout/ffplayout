import dayjs from 'dayjs'
import { isEqual } from 'es-toolkit/predicate'
import { differenceWith } from 'es-toolkit/array'
import { omit } from 'es-toolkit/object'
import utc from 'dayjs/plugin/utc.js'
import timezone from 'dayjs/plugin/timezone.js'

import { defineStore } from 'pinia'

import { authFetch } from '@/composables/authFetch'
import type { ScheduledPlaylistPosition } from '@/composables/livePlaylistSchedule'
import { i18n } from '@/i18n'
import { playlistOperations } from '@/composables/helper'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

dayjs.extend(utc)
dayjs.extend(timezone)

const { processPlaylist } = playlistOperations()

/** The fields an operator edits; ids and computed times do not count as changes. */
function editedContent(playlist: PlaylistItem[]): string {
    return JSON.stringify(
        playlist.map((item) => [
            item.source,
            item.in,
            item.out,
            item.duration,
            item.audio ?? '',
            item.title ?? '',
            item.ad ?? false,
            item.graphics ?? null,
        ]),
    )
}

export const usePlaylist = defineStore('playlist', {
    state: () => ({
        playlist: [] as PlaylistItem[],
        savedContent: editedContent([]),
        isLoading: true,
        listDate: dayjs().format('YYYY-MM-DD'),
        progressValue: 0,
        current: {} as PlaylistItem,
        currentIndex: 0,
        liveScheduledClip: undefined as ScheduledPlaylistPosition | undefined,
        ingestRuns: false,
        ingestInputs: [] as LiveInputStatus[],
        ingestListenerId: null as number | null,
        ingestListenerName: null as string | null,
        ingestListenerBackend: null as string | null,
        audioLevel: null as AudioLevel | null,
        elapsedSec: 0,
        shift: 0,
        playoutIsRunning: false,
        last_channel: 0,
        firstLoad: true,
        scrollToItem: false,
    }),

    getters: {
        /** The playlist differs from what was last loaded from or saved to the server. */
        hasUnsavedChanges: (state) => editedContent(state.playlist) !== state.savedContent,
    },
    actions: {
        /** Remember the current playlist as the state stored on the server. */
        markSaved() {
            this.savedContent = editedContent(this.playlist)
        },

        async getPlaylist(date: string) {
            const authStore = useAuth()
            const configStore = useConfig()
            const indexStore = useIndex()
            const channel = configStore.channels[configStore.i]?.id

            await authFetch<Playlist>(`/api/playlist/${channel}?date=${date}`, {
                method: 'GET',
                headers: authStore.authHeader,
            })
                .then((data: Playlist) => {
                    if (data.program) {
                        const programData = processPlaylist(date, data.program, false)

                        if (
                            channel === this.last_channel &&
                            this.playlist.length > 0 &&
                            programData.length > 0 &&
                            (this.playlist[0]?.date === date || configStore.playout.playlist.infinit) &&
                            differenceWith(this.playlist, programData, (a, b) => {
                                return isEqual(omit(a, ['uid']), omit(b, ['uid']))
                            }).length > 0
                        ) {
                            indexStore.msgAlert('warning', i18n.t('player.unsavedProgram'), 3)
                        } else {
                            this.playlist = programData ?? []
                            this.markSaved()
                        }
                    }
                })
                .catch((e) => {
                    if (e.status >= 400) {
                        indexStore.msgAlert('error', e.message, 5)

                        if (this.playlist[0]?.date !== date) {
                            this.playlist = []
                            this.markSaved()
                            this.markSaved()
                        }
                    } else if (
                        channel === this.last_channel &&
                        this.playlist.length > 0 &&
                        this.playlist[0]?.date === date
                    ) {
                        indexStore.msgAlert('warning', i18n.t('player.unsavedProgram'), 3)
                    } else {
                        this.playlist = []
                        this.markSaved()
                    }
                })

            this.last_channel = channel ?? 0
        },

        setStatus(item: PlayoutStatus) {
            this.playoutIsRunning = true
            this.current = item.media
            this.currentIndex = item.index
            this.elapsedSec = item.elapsed
            this.ingestRuns = item.ingest
            this.ingestInputs = item.ingest_inputs ?? []
            this.ingestListenerId = item.ingest ? (item.ingest_listener_id ?? null) : null
            this.ingestListenerName = item.ingest ? (item.ingest_listener_name ?? null) : null
            this.ingestListenerBackend = item.ingest ? (item.ingest_listener_backend ?? null) : null
            this.audioLevel = item.audio ?? null
            this.shift = item.shift

            this.progressValue = (this.elapsedSec * 100) / this.current.out
        },
    },
})
