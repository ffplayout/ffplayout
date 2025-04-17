<template>
    <div class="w-full">
        <div class="grid grid-cols-1 md:grid-cols-[auto_512px] xl:grid-cols-[512px_auto_450px]">
            <div class="order-1 p-1">
                <div class="bg-base-100 w-full h-full rounded-sm shadow-sm flex items-center p-2">
                    <div class="w-full aspect-video">
                        <video v-if="streamExtension === 'flv'" ref="httpStreamFlv" controls />
                        <VideoPlayer
                            v-else-if="configStore.showPlayer && configStore.channels[configStore.i]"
                            :key="configStore.i"
                            class="live-player"
                            reference="httpStream"
                            :options="{
                                liveui: true,
                                controls: true,
                                suppressNotSupportedError: true,
                                autoplay: false,
                                preload: 'auto',
                                sources: [
                                    {
                                        type: 'application/x-mpegURL',
                                        src: configStore.channels[configStore.i].preview_url,
                                    },
                                ],
                            }"
                        />
                    </div>
                </div>
            </div>

            <div
                class="order-3 xl:order-2 col-span-1 md:col-span-2 xl:col-span-1 bg-base-200 h-full grid grid-cols-1 xs:grid-cols-2"
            >
                <div class="col-span-1 p-1">
                    <div
                        class="w-full h-full bg-base-100 rounded-sm font-['DigitalNumbers'] p-6 text-3xl md:text-2xl 2xl:text-5xl 3xl:text-7xl tracking-tighter flex justify-center items-center shadow"
                    >
                        {{ timeStr }}
                    </div>
                </div>

                <div class="col-span-1 p-1 min-h-[50%]">
                    <div
                        class="w-full h-full bg-base-100 rounded-sm font-['DigitalNumbers'] p-6 text-3xl md:text-2xl 2xl:text-5xl 3xl:text-7xl tracking-tighter flex justify-center items-center shadow"
                    >
                        {{ secToHMS(timeRemaining()) }}
                    </div>
                </div>

                <div class="col-span-1 xs:col-span-2 p-1">
                    <div class="w-full h-full bg-base-100 rounded-sm flex items-center px-3 py-2 xl:py-1 shadow">
                        <div class="w-full h-full flex flex-col content-center">
                            <div v-if="playlistStore.ingestRuns" class="h-1/4 font-bold truncate leading-5">
                                {{ t('control.ingest') }}
                            </div>
                            <div
                                v-else
                                class="h-1/4 font-bold text truncate content-center leading-5"
                                :class="{ 'text-base-content/60': playlistStore.current.category === 'advertisement' }"
                                :title="playlistStore.current.title || filename(playlistStore.current.source)"
                            >
                                {{
                                    playlistStore.current.title ||
                                    filename(playlistStore.current.source) ||
                                    t('control.noClip')
                                }}
                            </div>
                            <div class="h-1/4 content-center leading-5 text-sm md:text-base">
                                <strong> {{ t('player.duration') }}: </strong>
                                {{ secToHMS(playlistStore.current.duration) }} |
                                <strong> {{ t('player.in') }}: </strong>
                                {{ secToHMS(playlistStore.current.in) }} |
                                <strong> {{ t('player.out') }}: </strong>
                                {{ secToHMS(playlistStore.current.out) }}

                                <template v-if="playlistStore.shift !== 0">
                                    | <strong>{{ t('player.shift') }}:</strong> {{ secToHMS(playlistStore.shift) }}
                                </template>
                            </div>
                            <div class="h-1/4 content-center leading-5 text-sm md:text-base">
                                <strong> {{ t('player.total') }}: </strong>
                                {{
                                    secToHMS(
                                        playlistStore.playlist.reduce(
                                            (total, { in: seek, out }) => total + (out - seek),
                                            0
                                        )
                                    )
                                }}
                            </div>
                            <div class="h-1/4 content-center">
                                <progress
                                    class="progress progress-accent w-full"
                                    :value="
                                        playlistStore.progressValue && playlistStore.progressValue <= 100
                                            ? playlistStore.progressValue
                                            : 0
                                    "
                                    max="100"
                                />
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div class="order-2 xl:order-3 p-1">
                <div class="bg-base-100 h-full flex justify-center rounded-sm shadow">
                    <div class="w-full h-full grid grid-cols-3">
                        <div class="text-center">
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.start')"
                                    class="btn btn-primary h-full w-full text-7xl text-lime-600"
                                    :class="playlistStore.playoutIsRunning && 'shadow-glow shadow-lime-600'"
                                    @click="controlProcess('start')"
                                >
                                    <i class="bi-play" />
                                </button>
                            </div>
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.last')"
                                    class="btn btn-primary h-full w-full text-7xl text-cyan-600"
                                    @click="controlPlayout('back')"
                                >
                                    <i class="bi-skip-start" />
                                </button>
                            </div>
                        </div>

                        <div class="text-center">
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.stop')"
                                    class="btn btn-primary h-full w-full text-7xl text-red-600"
                                    @click="controlProcess('stop')"
                                >
                                    <i class="bi-stop" />
                                </button>
                            </div>

                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.reset')"
                                    class="btn btn-primary h-full w-full text-6xl text-cyan-600"
                                    @click="controlPlayout('reset')"
                                >
                                    <i class="bi-arrow-repeat" />
                                </button>
                            </div>
                        </div>

                        <div class="text-center">
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.restart')"
                                    class="btn btn-primary h-full w-full text-6xl text-yellow-500"
                                    @click="controlProcess('restart')"
                                >
                                    <i class="bi-arrow-clockwise" />
                                </button>
                            </div>

                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    :title="t('control.next')"
                                    class="btn btn-primary h-full w-full text-7xl text-cyan-600"
                                    @click="controlPlayout('next')"
                                >
                                    <i class="bi-skip-end" />
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import mpegts from 'mpegts.js'

import { ref, onMounted, onBeforeUnmount, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { throttle } from 'lodash-es'
import { storeToRefs } from 'pinia'
import { useEventSource } from '@vueuse/core'

import { stringFormatter } from '@/composables/helper'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
import { usePlaylist } from '@/stores/playlist'

import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import VideoPlayer from '@/components/VideoPlayer.vue'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const playlistStore = usePlaylist()
const { filename, secToHMS } = stringFormatter()
const { i } = storeToRefs(useConfig())

const currentDefault = {
    uid: '',
    title: t('control.noClip'),
    source: t('control.noClip'),
    duration: 0,
    in: 0,
    out: 0,
} as PlaylistItem

playlistStore.current = currentDefault

const timeStr = ref('00:00:00')
const timer = ref()
const errorCounter = ref(0)
const streamExtension = ref(configStore.channels[configStore.i].preview_url.split('.').pop())
const httpStreamFlv = ref(null)
const httpFlvSource = ref({
    type: 'flv',
    isLive: true,
    url: configStore.channels[configStore.i].preview_url,
})
const mpegtsOptions = ref({
    lazyLoadMaxDuration: 3 * 60,
    liveBufferLatencyChasing: true,
})

const streamUrl = ref(`/data/event/${configStore.channels[configStore.i].id}?endpoint=playout&uuid=${authStore.uuid}`)

// 'http://127.0.0.1:8787/data/event/1?endpoint=playout&uuid=f2f8c29b-712a-48c5-8919-b535d3a05a3a'
const { status, data, error, close } = useEventSource(streamUrl, [], {
    autoReconnect: {
        retries: -1,
        delay: 1000,
        onFailed() {
            indexStore.sseConnected = false
        },
    },
})

onMounted(() => {
    let player: any = null

    if (streamExtension.value === 'flv' && mpegts.getFeatureList().mseLivePlayback) {
        if (typeof player !== 'undefined' && player != null) {
            player.unload()
            player.detachMediaElement()
            player.destroy()
            player = null
        }

        player = mpegts.createPlayer(httpFlvSource.value, mpegtsOptions.value)
        player.attachMediaElement(httpStreamFlv.value)
        player.load()
    }

    clock()
})

onBeforeUnmount(() => {
    indexStore.sseConnected = false
    close()

    if (timer.value) {
        clearTimeout(timer.value)
    }
})

watch([status, error], async () => {
    if (status.value === 'OPEN') {
        indexStore.sseConnected = true
        errorCounter.value = 0
    } else {
        indexStore.sseConnected = false
        errorCounter.value += 1

        if (errorCounter.value > 11) {
            await authStore.obtainUuid()
            streamUrl.value = `/data/event/${configStore.channels[configStore.i].id}?endpoint=playout&uuid=${
                authStore.uuid
            }`
            errorCounter.value = 0
        }
    }
})

watch([data], () => {
    if (data.value) {
        try {
            const playout_status = JSON.parse(data.value)
            playlistStore.setStatus(playout_status)
        } catch {
            indexStore.sseConnected = true
            playlistStore.playoutIsRunning = false
            resetStatus()
        }
    }
})

watch([i], () => {
    resetStatus()

    streamUrl.value = `/data/event/${configStore.channels[configStore.i].id}?endpoint=playout&uuid=${authStore.uuid}`

    if (timer.value) {
        clearTimeout(timer.value)
    }
})

function timeRemaining() {
    let remaining = playlistStore.current.out - playlistStore.elapsedSec

    if (remaining < 0) {
        remaining = 0
    }

    return remaining
}

async function clock() {
    async function setTime(resolve: any) {
        timeStr.value = dayjs().tz(configStore.timezone).format('HH:mm:ss')
        timer.value = setTimeout(() => setTime(resolve), 1000)
    }
    return new Promise((resolve) => setTime(resolve))
}

function resetStatus() {
    playlistStore.elapsedSec = 0
    playlistStore.shift = 0
    playlistStore.current = currentDefault
}

const controlProcess = throttle(async (state: string) => {
    /*
        Control playout (start, stop, restart)
    */
    const channel = configStore.channels[configStore.i].id
    await fetch(`/api/control/${channel}/process/`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify({ command: state }),
    })
        .then(() => {
            if (state === 'start') {
                playlistStore.scrollToItem = true
            }
        })
        .catch((e) => {
            indexStore.msgAlert('error', e.data, 3)
        })
}, 2000)

const controlPlayout = throttle(async (state: string) => {
    /*
        Control playout:
        - jump to next clip
        - jump to last clip
        - reset playout state
    */
    const channel = configStore.channels[configStore.i].id

    await fetch(`/api/control/${channel}/playout/`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify({ control: state }),
    }).catch((e) => {
        indexStore.msgAlert('error', e.data, 3)
    })
}, 1000)
</script>
