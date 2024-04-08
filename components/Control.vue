<template>
    <div class="w-full">
        <div class="grid grid-cols-1 md:grid-cols-[auto_512px] xl:grid-cols-[512px_auto_450px]">
            <div class="order-1 p-1 flex">
                <div class="aspect-video w-full">
                    <video v-if="streamExtension === 'flv'" ref="httpStreamFlv" controls />
                    <VideoPlayer
                        class="live-player"
                        v-else-if="configStore.configGui[configStore.configID]"
                        :key="configStore.configID"
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
                                    src: configStore.configGui[configStore.configID].preview_url,
                                },
                            ],
                        }"
                    />
                </div>
            </div>

            <div
                class="order-3 xl:order-2 col-span-1 md:col-span-2 xl:col-span-1 bg-base-200 h-full grid grid-cols-1 xs:grid-cols-2"
            >
                <div class="col-span-1 p-1">
                    <div
                        class="w-full h-full bg-base-100 rounded font-['DigitalNumbers-Regular'] p-6 text-3xl 2xl:text-5xl 4xl:text-7xl tracking-tighter flex justify-center items-center"
                    >
                        {{ timeStr }}
                    </div>
                </div>

                <div class="col-span-1 p-1 min-h-[50%]">
                    <div
                        class="w-full h-full bg-base-100 rounded font-['DigitalNumbers-Regular'] p-6 text-3xl 2xl:text-5xl 4xl:text-7xl tracking-tighter flex justify-center items-center"
                    >
                        {{ secToHMS(playlistStore.remainingSec >= 0 ? playlistStore.remainingSec : 0) }}
                    </div>
                </div>

                <div class="col-span-1 xs:col-span-2 p-1">
                    <div class="w-full h-full bg-base-100 flex items-center p-3">
                        <div class="w-full h-full flex flex-col">
                            <div v-if="playlistStore.ingestRuns" class="h-1/3 font-bold truncate" title="Live Ingest">
                                Live Ingest
                            </div>
                            <div
                                v-else
                                class="h-1/3 font-bold text truncate"
                                :title="filename(playlistStore.currentClip)"
                            >
                                {{ filename(playlistStore.currentClip) }}
                            </div>
                            <div class="grow">
                                <strong>Duration:</strong> {{ secToHMS(playlistStore.currentClipDuration) }} |
                                <strong>In:</strong> {{ secToHMS(playlistStore.currentClipIn) }} | <strong>Out:</strong>
                                {{ secToHMS(playlistStore.currentClipOut) }}
                            </div>
                            <div class="h-1/3">
                                <progress
                                    class="progress progress-accent w-full"
                                    :value="playlistStore.progressValue"
                                    max="100"
                                ></progress>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <div class="order-2 xl:order-3 p-1">
                <div class="bg-base-100 h-full flex justify-center">
                    <div class="w-full h-full grid grid-cols-3">
                        <div class="text-center">
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    title="Start Playout Service"
                                    class="btn btn-primary h-full w-full text-7xl text-lime-600"
                                    :class="playlistStore.playoutIsRunning && 'shadow-glow shadow-lime-600'"
                                    @click="controlProcess('start')"
                                >
                                    <i class="bi-play" />
                                </button>
                            </div>
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    title="Jump to last Clip"
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
                                    title="Stop Playout Service"
                                    class="btn btn-primary h-full w-full text-7xl text-red-600"
                                    @click="controlProcess('stop')"
                                >
                                    <i class="bi-stop" />
                                </button>
                            </div>

                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    title="Reset Playout State"
                                    class="btn btn-primary h-full w-full text-7xl text-cyan-600"
                                    @click="controlPlayout('reset')"
                                >
                                    <i class="bi-arrow-repeat" />
                                </button>
                            </div>
                        </div>

                        <div class="text-center">
                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    title="Restart Playout Service"
                                    class="btn btn-primary h-full w-full text-7xl text-yellow-500"
                                    @click="controlProcess('restart')"
                                >
                                    <i class="bi-arrow-clockwise" />
                                </button>
                            </div>

                            <div class="w-full h-1/2 aspect-square p-2">
                                <button
                                    title="Jump to next Clip"
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
import { storeToRefs } from 'pinia'
import mpegts from 'mpegts.js'

const { $dayjs } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const playlistStore = usePlaylist()
const { filename, secToHMS, timeToSeconds } = stringFormatter()
const { configID } = storeToRefs(useConfig())
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const breakStatusCheck = ref(false)
const timeStr = ref('00:00:00')
const timer = ref()
const streamExtension = ref(configStore.configGui[configStore.configID].preview_url.split('.').pop())
const httpStreamFlv = ref(null)
const httpFlvSource = ref({
    type: 'flv',
    isLive: true,
    url: configStore.configGui[configStore.configID].preview_url,
})
const mpegtsOptions = ref({
    lazyLoadMaxDuration: 3 * 60,
    liveBufferLatencyChasing: true,
})

onMounted(() => {
    breakStatusCheck.value = false
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

    status()
})

watch([configID], () => {
    breakStatusCheck.value = false
    timeStr.value = '00:00:00'
    playlistStore.remainingSec = -1
    playlistStore.currentClip = ''
    playlistStore.ingestRuns = false
    playlistStore.currentClipDuration = 0
    playlistStore.currentClipIn = 0
    playlistStore.currentClipOut = 0

    if (timer.value) {
        clearTimeout(timer.value)
    }
    status()
})

onBeforeUnmount(() => {
    breakStatusCheck.value = true

    if (timer.value) {
        clearTimeout(timer.value)
    }
})

async function status() {
    /*
        Get playout state and information's from current clip.
        - animate timers
        - when clip end is reached call API again and set new values
    */
    await playlistStore.playoutStat()

    async function setStatus(resolve: any) {
        /*
            recursive function as a endless loop
        */
        timeStr.value = $dayjs().utcOffset(configStore.utcOffset).format('HH:mm:ss')
        const timeInSec = timeToSeconds(timeStr.value)
        playlistStore.remainingSec = playlistStore.currentClipStart + playlistStore.currentClipOut - timeInSec
        const playedSec = playlistStore.currentClipOut - playlistStore.remainingSec
        playlistStore.progressValue = (playedSec * 100) / playlistStore.currentClipOut

        if (breakStatusCheck.value) {
            return
        } else if ((playlistStore.playoutIsRunning && playlistStore.remainingSec < 0) || timeInSec % 30 === 0) {
            // When 30 seconds a passed, get new status.
            await playlistStore.playoutStat()
        } else if (!playlistStore.playoutIsRunning) {
            playlistStore.remainingSec = 0
        }

        timer.value = setTimeout(() => setStatus(resolve), 1000)
    }
    return new Promise((resolve) => setStatus(resolve))
}

async function controlProcess(state: string) {
    /*
        Control playout systemd service (start, stop, restart)
    */
    const channel = configStore.configGui[configStore.configID].id

    await $fetch(`/api/control/${channel}/process/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ command: state }),
    })

    setTimeout(async () => {
        await playlistStore.playoutStat()
    }, 1000)
}

async function controlPlayout(state: string) {
    /*
        Control playout:
        - jump to next clip
        - jump to last clip
        - reset playout state
    */
    const channel = configStore.configGui[configStore.configID].id

    await $fetch(`/api/control/${channel}/playout/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ control: state }),
    })

    setTimeout(async () => {
        await playlistStore.playoutStat()
    }, 1000)
}
</script>

<style lang="scss" scoped>
.control-row {
    min-height: 254px;
}

.player-col {
    max-width: 542px;
    min-width: 380px;
    padding-top: 2px;
    padding-bottom: 2px;
}

.player-col > div {
    background-color: black;
    width: 100%;
    height: 100%;
}

.live-player {
    position: relative;
    top: 50%;
    transform: translateY(-50%);
}

.control-col {
    height: 100%;
    min-height: 254px;
}

.status-col {
    padding-right: 30px;
}

.control-unit-col {
    min-width: 250px;
    padding: 2px 17px 2px 2px;
}

.control-unit-row {
    background: $gray-900;
    height: 100%;
    margin-right: 0;
    border-radius: 0.25rem;
    text-align: center;
}

.control-unit-row .col {
    height: 50%;
    min-height: 90px;
}

.control-unit-row .col div {
    height: 80%;
    margin: 0.6em 0;
}

.control-button {
    font-size: 4em;
    line-height: 0;
    width: 80%;
    height: 100%;
}

.status-row {
    height: 100%;
    min-width: 325px;
}

.status-row .col {
    margin: 2px;
}

.time-col {
    position: relative;
    background: $gray-900;
    padding: 0.5em;
    text-align: center;
    border-radius: 0.25rem;
}

.time-str {
    position: relative;
    top: 50%;
    -webkit-transform: translateY(-50%);
    -ms-transform: translateY(-50%);
    transform: translateY(-50%);
    font-family: 'DigitalNumbers-Regular';
    font-size: 4.5em;
    letter-spacing: -0.18em;
    padding-right: 14px;
}

.current-clip {
    background: $gray-900;
    padding: 10px;
    border-radius: 0.25rem;
    min-width: 700px;
}

.current-clip-text {
    height: 40%;
    padding-top: 0.5em;
    text-align: left;
    font-weight: bold;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
}

.current-clip-meta {
    margin-bottom: 0.7em;
}

.current-clip-progress {
    top: 80%;
    margin-top: 0.2em;
}

.control-button-play {
    color: $control-button-play;

    &:hover {
        color: $control-button-play-hover;
    }
}
.is-playing {
    box-shadow: 0 0 15px $control-button-play;
}

.control-button-stop {
    color: $control-button-stop;

    &:hover {
        color: $control-button-stop-hover;
    }
}

.control-button-restart {
    color: $control-button-restart;

    &:hover {
        color: $control-button-restart-hover;
    }
}

.control-button-control {
    color: $control-button-control;

    &:hover {
        color: $control-button-control-hover;
    }
}

.clip-progress {
    height: 5px;
    padding-top: 3px;
}

@media (max-width: 1555px) {
    .control-row {
        min-height: 200px;
    }

    .control-col {
        height: 100%;
        min-height: inherit;
    }
    .status-col {
        padding-right: 0;
        height: 100%;
        flex: 0 0 60%;
        max-width: 60%;
    }
    .current-clip {
        min-width: 300px;
    }
    .time-str {
        font-size: 3.5em;
    }
    .control-unit-row {
        margin-right: -30px;
    }
    .control-unit-col {
        flex: 0 0 35%;
        max-width: 35%;
        margin: 0 0 0 30px;
    }
}

@media (max-width: 1337px) {
    .status-col {
        flex: 0 0 47%;
        max-width: 47%;
        height: 68%;
    }
    .control-unit-col {
        flex: 0 0 47%;
        max-width: 47%;
    }
}

@media (max-width: 1102px) {
    .control-unit-row .col {
        min-height: 70px;
        padding-right: 0;
        padding-left: 0;
    }
    .control-button {
        font-size: 2em;
    }
}

@media (max-width: 889px) {
    .control-row {
        min-height: 540px;
    }

    .status-col {
        flex: 0 0 94%;
        max-width: 94%;
        height: 68%;
    }
    .control-unit-col {
        flex: 0 0 94%;
        max-width: 94%;
        margin: 0;
        padding-left: 17px;
    }
}

@media (max-width: 689px) {
    .player-col {
        flex: 0 0 98%;
        max-width: 98%;
        padding-top: 30px;
    }
    .control-row {
        min-height: 830px;
    }
    .control-col {
        margin: 0;
    }
    .control-unit-col,
    .status-col {
        flex: 0 0 96%;
        max-width: 96%;
    }
}
</style>
