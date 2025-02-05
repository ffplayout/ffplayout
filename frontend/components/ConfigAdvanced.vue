<template>
    <div class="max-w-[1200px] xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('advanced.title') }}</h2>
        <p class="mt-5 font-bold text-orange-500">{{ t('advanced.warning') }}</p>
        <div>
            <label class="form-control w-full max-w-xs mt-2">
                <div class="label">
                    <span class="label-text !text-md font-bold">Preset</span>
                </div>
                <div class="flex-none join">
                    <select v-model="configStore.advanced" class="join-item select select-sm select-bordered w-full">
                        <option v-for="config in relatedConfigs" :key="config.id" :value="config">
                            {{ config.name }}
                        </option>
                    </select>
                    <button
                        class="join-item btn btn-sm btn-primary select-bordered"
                        title="Add preset"
                        @click="addAdvancedConfig()"
                    >
                        <i class="bi-plus-lg" />
                    </button>
                    <button
                        class="join-item btn btn-sm btn-primary select-bordered"
                        title="Delete preset"
                        @click="removeAdvancedConfig()"
                    >
                        <i class="bi-x-lg" />
                    </button>
                </div>
            </label>
        </div>
        <form
            v-if="configStore.advanced"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitAdvanced"
        >
            <div class="text-xl pt-3 md:text-right">Name:</div>
            <div>
                <label class="form-control w-full mt-4">
                    <input
                        v-model="configStore.advanced.name"
                        type="text"
                        name="input_param"
                        class="input input-sm input-bordered w-full max-w-64"
                    />
                </label>
            </div>
            <div class="text-xl pt-3 md:text-right">{{ t('advanced.decoder') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        In streaming mode, the decoder settings are responsible for unifying the media files.
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Input Parameter</span>
                    </div>
                    <input
                        v-model="configStore.advanced.decoder.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm input-bordered w-full"
                    />
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Output Parameter</span>
                    </div>
                    <input
                        v-model="configStore.advanced.decoder.output_param"
                        type="text"
                        name="output_param"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default:
                            <span class="select-text cursor-text">
                                -c:v mpeg2video -g 1 -b:v 57600k -minrate 57600k -maxrate 57600k -bufsize 28800k -c:a
                                s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2
                            </span>
                        </span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.encoder') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">Encoder settings representing the streaming output.</div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Input Parameter</span>
                    </div>
                    <input
                        v-model="configStore.advanced.encoder.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm input-bordered w-full"
                    />
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.filter') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        The filters are mainly there to transform audio and video into the correct format, but also to
                        place text and logo over the video, create in/out fade etc.<br />

                        If curly brackets are included in the default values, these must be adopted.
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Deinterlace</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.deinterlace"
                        type="text"
                        name="deinterlace"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80"
                            >Default: <span class="select-text cursor-text">yadif=0:-1:0 </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Pad Video</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.pad_video"
                        type="text"
                        name="pad_video"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default:
                            <span class="select-text cursor-text"> pad='ih*{}/{}:ih:(ow-iw)/2:(oh-ih)/2' </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">FPS</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.fps"
                        type="text"
                        name="fps"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">fps={} </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Scale</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.scale"
                        type="text"
                        name="scale"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">scale={}:{} </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Set Dar</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.set_dar"
                        type="text"
                        name="set_dar"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">setdar=dar={} </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Fade In</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.fade_in"
                        type="text"
                        name="fade_in"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">fade=in:st=0:d=0.5 </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Fade Out</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.fade_out"
                        type="text"
                        name="fade_out"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">fade=out:st={}:d=1.0 </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.logo"
                        type="text"
                        name="logo"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default:
                            <span class="select-text cursor-text">
                                movie={}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa={}
                            </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Scale</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_scale"
                        type="text"
                        name="overlay_logo_scale"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">scale={}</span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Fade In</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_fade_in"
                        type="text"
                        name="overlay_logo_fade_in"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">fade=in:st=0:d=1.0:alpha=1</span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Fade Out</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_fade_out"
                        type="text"
                        name="overlay_logo_fade_out"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">fade=out:st={}:d=1.0:alpha=1</span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Logo Overlay</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo"
                        type="text"
                        name="overlay_logo"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">overlay={}:shortest=1</span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">TPad</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.tpad"
                        type="text"
                        name="tpad"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">tpad=stop_mode=add:stop_duration={}</span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Drawtext from File</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.drawtext_from_file"
                        type="text"
                        name="drawtext_from_file"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">drawtext=text='{}':{}{} </span></span
                        >
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Drawtext from ZMQ</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.drawtext_from_zmq"
                        type="text"
                        name="drawtext_from_zmq"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default:
                            <span class="select-text cursor-text">zmq=b=tcp\\\\://'{}',drawtext@dyntext={}</span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Source</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.aevalsrc"
                        type="text"
                        name="aevalsrc"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80 break-all">
                            Default: aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Fade In</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.afade_in"
                        type="text"
                        name="afade_in"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">afade=in:st=0:d=0.5 </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Fade Out</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.afade_out"
                        type="text"
                        name="afade_out"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">afade=out:st={}:d=1.0 </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Audio Pad</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.apad"
                        type="text"
                        name="apad"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">apad=whole_dur={} </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Volumen</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.volume"
                        type="text"
                        name="volume"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">volume={} </span>
                        </span>
                    </div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Split</span>
                    </div>
                    <input
                        v-model="configStore.advanced.filter.split"
                        type="text"
                        name="split"
                        class="input input-sm input-bordered w-full"
                    />
                    <div class="label">
                        <span class="text-sm text-base-content/80">
                            Default: <span class="select-text cursor-text">split={}{} </span>
                        </span>
                    </div>
                </label>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.ingest') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">Ingest settings are for live streaming input.</div>
                </label>
                <label class="form-control w-full mt-2">
                    <div class="label">
                        <span class="label-text !text-md font-bold">Input Parameter</span>
                    </div>
                    <input
                        v-model="configStore.advanced.ingest.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm input-bordered w-full"
                    />
                </label>
            </div>

            <div class="mt-5 mb-10">
                <button class="btn btn-primary" type="submit">{{ t('config.save') }}</button>
            </div>
        </form>
    </div>

    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="showModal"
        :modal-action="restart"
    />
</template>

<script setup lang="ts">
import { cloneDeep } from 'lodash-es'
import type { AdvancedConfig } from '~/types/advanced_config'

const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const showModal = ref(false)
const relatedConfigs = ref<AdvancedConfig[]>([])

const newConfig = {
    id: 0,
    name: null,
    decoder: {
        input_param: '',
        output_param: '',
    },
    encoder: {
        input_param: '',
    },
    filter: {
        deinterlace: '',
        pad_scale_w: '',
        pad_scale_h: '',
        pad_video: '',
        fps: '',
        scale: '',
        set_dar: '',
        fade_in: '',
        fade_out: '',
        logo: '',
        overlay_logo_scale: '',
        overlay_logo_fade_in: '',
        overlay_logo_fade_out: '',
        overlay_logo: '',
        tpad: '',
        drawtext_from_file: '',
        drawtext_from_zmq: '',
        aevalsrc: '',
        afade_in: '',
        afade_out: '',
        apad: '',
        volume: '',
        split: '',
    },
    ingest: {
        input_param: '',
    },
}

onBeforeMount(async () => {
    await fetchRelatedConfigs()
})

async function fetchRelatedConfigs() {
    const id = configStore.channels[configStore.i].id

    await $fetch(`/api/playout/advanced/${id}/`, {
        method: 'GET',
        headers: { ...configStore.contentType, ...authStore.authHeader },
    })
        .then((response: any) => {
            relatedConfigs.value = response
        })
        .catch((e) => {
            indexStore.msgAlert('error', e, 3)
        })
}

function addAdvancedConfig() {
    configStore.advanced = cloneDeep(newConfig)
}

async function removeAdvancedConfig() {
    const id = configStore.channels[configStore.i].id

    await $fetch(`/api/playout/advanced/${id}/${configStore.advanced.id}`, {
        method: 'DELETE',
        headers: { ...configStore.contentType, ...authStore.authHeader },
    })
        .then(async () => {
            await fetchRelatedConfigs()
            configStore.advanced = cloneDeep(newConfig)
        })
        .catch((e) => {
            indexStore.msgAlert('error', e, 3)
        })
}

async function onSubmitAdvanced() {
    const update = await configStore.setAdvancedConfig()
    configStore.onetimeInfo = true

    if (update.status === 200) {
        const id = configStore.channels[configStore.i].id
        indexStore.msgAlert('success', t('advanced.updateSuccess'), 2)

        await fetchRelatedConfigs()

        await $fetch(`/api/control/${id}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        }).then((response: any) => {
            if (response === 'active') {
                showModal.value = true
            }
        })
    } else {
        indexStore.msgAlert('error', t('advanced.updateFailed'), 2)
    }
}

async function restart(res: boolean) {
    if (res) {
        const channel = configStore.channels[configStore.i].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'restart' }),
        })
    }

    showModal.value = false
}
</script>
