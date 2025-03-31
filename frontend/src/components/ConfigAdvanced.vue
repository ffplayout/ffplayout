<template>
    <div class="max-w-[1200px] xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('advanced.title') }}</h2>
        <p class="mt-5 font-bold text-orange-500">{{ t('advanced.warning') }}</p>
        <div class="mt-4">
            <label class="form-control">
                <div class="label w-full">
                    <span class="label-text !text-md font-bold">Preset</span>
                </div>
                <div class="flex-none join">
                    <select
                        v-model="configStore.advanced"
                        class="join-item select select-sm min-w-44 focus:border-base-content/30 focus:outline-base-content/30"
                    >
                        <option v-for="config in relatedConfigs" :key="config.id" :value="config">
                            {{ config.name }}
                        </option>
                    </select>
                    <button class="join-item btn btn-sm btn-primary" title="Add preset" @click="addAdvancedConfig()">
                        <i class="bi-plus-lg" />
                    </button>
                    <button
                        class="join-item btn btn-sm btn-primary"
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
            <div class="text-xl md:text-right">Name:</div>
            <fieldset class="fieldset">
                <input
                    v-model="configStore.advanced.name"
                    type="text"
                    name="name"
                    class="input input-sm w-full xs:max-w-64"
                />
            </fieldset>
            <div class="text-xl pt-3 md:text-right">{{ t('advanced.decoder') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        In streaming mode, the decoder settings are responsible for unifying the media files.
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Input Parameter</legend>
                    <input
                        v-model="configStore.advanced.decoder.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm w-full"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Output Parameter</legend>
                    <input
                        v-model="configStore.advanced.decoder.output_param"
                        type="text"
                        name="output_param"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>-c:v mpeg2video -g 1 -b:v 57600k -minrate 57600k
                        -maxrate 57600k -bufsize 28800k -c:a s302m -strict -2 -sample_fmt s16 -ar 48000 -ac 2
                    </p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.encoder') }}:</div>
            <div class="md:pt-4">
                <label class="form-control">
                    <div class="whitespace-pre-line">Encoder settings representing the streaming output.</div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Input Parameter</legend>
                    <input
                        v-model="configStore.advanced.encoder.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm w-full"
                    />
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.filter') }}:</div>
            <div class="md:pt-4">
                <label class="form-control">
                    <div class="whitespace-pre-line">
                        The filters are mainly there to transform audio and video into the correct format, but also to
                        place text and logo over the video, create in/out fade etc.<br />

                        If curly brackets are included in the default values, these must be adopted.
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Deinterlace</legend>
                    <input
                        v-model="configStore.advanced.filter.deinterlace"
                        type="text"
                        class="input input-sm w-full"
                        name="deinterlace"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>yadif=0:-1:0</p>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Pad Video</legend>
                    <input
                        v-model="configStore.advanced.filter.pad_video"
                        type="text"
                        class="input input-sm w-full"
                        name="pad_video"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>pad='ih*{}/{}:ih:(ow-iw)/2:(oh-ih)/2'
                    </p>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">FPS</legend>
                    <input
                        v-model="configStore.advanced.filter.fps"
                        type="text"
                        name="fps"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>fps={}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Scale</legend>
                    <input
                        v-model="configStore.advanced.filter.scale"
                        type="text"
                        name="scale"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>scale={}:{}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Set Dar</legend>
                    <input
                        v-model="configStore.advanced.filter.set_dar"
                        type="text"
                        name="set_dar"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>setdar=dar={}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Fade In</legend>
                    <input
                        v-model="configStore.advanced.filter.fade_in"
                        type="text"
                        name="fade_in"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>fade=in:st=0:d=0.5
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Fade Out</legend>
                    <input
                        v-model="configStore.advanced.filter.fade_out"
                        type="text"
                        name="fade_out"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>fade=out:st={}:d=1.0
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo</legend>
                    <input
                        v-model="configStore.advanced.filter.logo"
                        type="text"
                        name="logo"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>
                        <span class="break-all">
                            movie={}:loop=0,setpts=N/(FRAME_RATE*TB),format=rgba,colorchannelmixer=aa={}
                        </span>
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Scale</legend>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_scale"
                        type="text"
                        name="overlay_logo_scale"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>scale={}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Fade In</legend>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_fade_in"
                        type="text"
                        name="overlay_logo_fade_in"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>fade=in:st=0:d=1.0:alpha=1
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Fade Out</legend>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo_fade_out"
                        type="text"
                        name="overlay_logo_fade_out"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>fade=out:st={}:d=1.0:alpha=1
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Overlay</legend>
                    <input
                        v-model="configStore.advanced.filter.overlay_logo"
                        type="text"
                        name="overlay_logo"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>overlay={}:shortest=1
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">TPad</legend>
                    <input
                        v-model="configStore.advanced.filter.tpad"
                        type="text"
                        name="tpad"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>tpad=stop_mode=add:stop_duration={}
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Drawtext from File</legend>
                    <input
                        v-model="configStore.advanced.filter.drawtext_from_file"
                        type="text"
                        name="drawtext_from_file"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>drawtext=text='{}':{}{}
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Drawtext from ZMQ</legend>
                    <input
                        v-model="configStore.advanced.filter.drawtext_from_zmq"
                        type="text"
                        name="drawtext_from_zmq"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>
                        <span class="break-all">zmq=b=tcp\\\\://'{}',drawtext@dyntext={}</span>
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Source</legend>
                    <input
                        v-model="configStore.advanced.filter.aevalsrc"
                        type="text"
                        name="aevalsrc"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>
                        <span class="break-all">aevalsrc=0:channel_layout=stereo:duration={}:sample_rate=48000</span>
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Fade In</legend>
                    <input
                        v-model="configStore.advanced.filter.afade_in"
                        type="text"
                        name="afade_in"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>afade=in:st=0:d=0.5
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Fade Out</legend>
                    <input
                        v-model="configStore.advanced.filter.afade_out"
                        type="text"
                        name="afade_out"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>afade=out:st={}:d=1.0
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Pad</legend>
                    <input
                        v-model="configStore.advanced.filter.apad"
                        type="text"
                        name="apad"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline">
                        <span class="font-bold">Default: </span>apad=whole_dur={}
                    </p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Volumen</legend>
                    <input
                        v-model="configStore.advanced.filter.volume"
                        type="text"
                        name="volume"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>volume={}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Split</legend>
                    <input
                        v-model="configStore.advanced.filter.split"
                        type="text"
                        name="split"
                        class="input input-sm w-full"
                    />
                    <p class="fieldset-label items-baseline"><span class="font-bold">Default: </span>split={}{}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('advanced.ingest') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">Ingest settings are for live streaming input.</div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Input Parameter</legend>
                    <input
                        v-model="configStore.advanced.ingest.input_param"
                        type="text"
                        name="input_param"
                        class="input input-sm w-full"
                    />
                </fieldset>
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
import { ref, onBeforeMount } from 'vue'
import { useI18n } from 'vue-i18n'
import { cloneDeep } from 'lodash-es'
import type { AdvancedConfig } from '@/types/advanced_config'

import GenericModal from '@/components/GenericModal.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

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

    await fetch(`/api/playout/advanced/${id}/`, {
        method: 'GET',
        headers: { ...configStore.contentType, ...authStore.authHeader },
    })
        .then(async (response) => {
            if (!response.ok) {
                throw new Error(await response.text())
            }

            return response.json()
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

    await fetch(`/api/playout/advanced/${id}/${configStore.advanced.id}`, {
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

        await fetch(`/api/control/${id}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        .then(async (response) => {
            if (!response.ok) {
                throw new Error(await response.text())
            }

            return response.json()
        })
        .then((response: any) => {
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

        await fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'restart' }),
        })
    }

    showModal.value = false
}
</script>
