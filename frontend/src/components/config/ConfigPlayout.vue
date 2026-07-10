<script setup lang="ts">
import { computed, watch } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const logLevels = ['INFO', 'WARNING', 'ERROR']
const processingMode = ['folder', 'playlist']
const videoPresets = [
    'ultrafast',
    'superfast',
    'veryfast',
    'faster',
    'fast',
    'medium',
    'slow',
    'slower',
    'veryslow',
    'placebo',
]
function outputMode(value: string | undefined): 'desktop' | 'hls' | 'stream' {
    if (value === 'desktop' || value === 'hls' || value === 'stream') {
        return value
    }

    return 'stream'
}

const extensions = computed({
    get() {
        return configStore.playout.storage.extensions.join(',')
    },

    set(value: string) {
        configStore.playout.storage.extensions = value.replace(' ', '').split(/,|;/)
    },
})

const output = computed({
    get() {
        return configStore.outputs.find((o) => o.id === configStore.playout.output.id)?.name
    },

    set(value: string) {
        const output = configStore.outputs.find((o) => o.name === value)
        configStore.playout.output.id = output?.id ?? 0
        configStore.playout.output.mode = outputMode(output?.name)
        configStore.playout.output.stream_url = output?.stream_url ?? ''
        configStore.playout.output.stream_type = output?.stream_type ?? 'rtmp'
        configStore.playout.output.hls_playlist_name = output?.hls_playlist_name ?? 'stream'
        configStore.playout.output.hls_segment_duration = output?.hls_segment_duration ?? 6
        configStore.playout.output.hls_list_size = output?.hls_list_size ?? 600
        configStore.playout.output.desktop_fullscreen = output?.desktop_fullscreen ?? false
        configStore.playout.output.width = output?.width ?? 1280
        configStore.playout.output.height = output?.height ?? 720
        configStore.playout.output.fps = output?.fps ?? 25
        configStore.playout.output.video_preset = output?.video_preset ?? 'faster'
        configStore.playout.output.video_codec = output?.video_codec ?? 'libx264'
        configStore.playout.output.audio_codec = output?.audio_codec ?? 'aac'
        configStore.playout.output.rate_control = output?.rate_control ?? 'crf'
        configStore.playout.output.video_quality = output?.video_quality ?? 23
        configStore.playout.output.video_maxrate = output?.video_maxrate ?? 2400
        configStore.playout.output.audio_bitrate = output?.audio_bitrate ?? 128
        configStore.playout.output.hls_variants = (output?.hls_variants ?? '')
            .split(';')
            .map((v) => v.trim())
            .filter((v) => v.length > 0)
    },
})

interface HlsVariantRow {
    name: string
    width: string
    height: string
    videoBitrate: string
    audioBitrate: string
}

function parseHlsVariant(spec: string): HlsVariantRow {
    const [name = '', resolution = '', videoBitrate = '', audioBitrate = ''] = spec.split(':')
    const [width = '', height = ''] = resolution.split('x')

    return { name, width, height, videoBitrate, audioBitrate }
}

function serializeHlsVariant(row: HlsVariantRow): string {
    const base = `${row.name}:${row.width}x${row.height}:${row.videoBitrate}`
    return row.audioBitrate ? `${base}:${row.audioBitrate}` : base
}

const hlsVariants = computed<HlsVariantRow[]>({
    get() {
        return configStore.playout.output.hls_variants.map(parseHlsVariant)
    },

    set(rows: HlsVariantRow[]) {
        configStore.playout.output.hls_variants = rows.map(serializeHlsVariant)
    },
})

const codecOptions = computed<OutputCodecOptions>(() => {
    if (output.value === 'stream') {
        return configStore.outputCodecs[configStore.playout.output.stream_type ?? 'rtmp']
    }

    return configStore.outputCodecs.hls
})

function codecLabel(codec: CodecOption): string {
    const label = codec.display_name || codec.name
    const details = codec.hardware ? `${codec.codec_id}, hardware` : codec.codec_id

    return `${codec.name} - ${label} (${details})`
}

watch(
    () => [output.value, configStore.playout.output.stream_type, codecOptions.value],
    () => {
        const video = codecOptions.value.video
        const audio = codecOptions.value.audio

        if (video.length > 0 && !video.some((codec) => codec.name === configStore.playout.output.video_codec)) {
            configStore.playout.output.video_codec = video[0].name
        }
        if (audio.length > 0 && !audio.some((codec) => codec.name === configStore.playout.output.audio_codec)) {
            configStore.playout.output.audio_codec = audio[0].name
        }
    },
)

function addHlsVariant() {
    hlsVariants.value = [
        ...hlsVariants.value,
        { name: '', width: '1280', height: '720', videoBitrate: '2500k', audioBitrate: '128k' },
    ]
}

function removeHlsVariant(index: number) {
    const rows = [...hlsVariants.value]
    rows.splice(index, 1)
    hlsVariants.value = rows
}

function updateHlsVariant(index: number, field: keyof HlsVariantRow, value: string) {
    const rows = [...hlsVariants.value]
    rows[index] = { ...rows[index], [field]: value }
    hlsVariants.value = rows
}

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

async function applyVolume() {
    try {
        const response = await configStore.applyAudioEffects(configStore.playout.processing.volume)
        if (!response.ok) {
            throw new Error(await response.text())
        }
        indexStore.msgAlert('success', t('config.volumeApplied'), 2)
    } catch {
        indexStore.msgAlert('error', t('config.volumeApplyFailed'), 3)
    }
}

async function onSubmitPlayout() {
    const { requiresRestart, volumeChanged } = configStore.playoutChangeSummary()
    const update = await configStore.setPlayoutConfig(configStore.playout)
    configStore.onetimeInfo = true

    if (update.status === 200) {
        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        if (volumeChanged) {
            await applyVolume()
        }

        const id = configStore.channels[configStore.i]?.id

        await fetch(`/api/control/${id}/process`, {
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
            .then(async (response) => {
                if (response === 'active' && requiresRestart) {
                    configStore.showRestartModal = true
                }

                await configStore.getPlayoutConfig()
                await configStore.getPlayoutOutputs()
            })
            .catch((e) => {
                indexStore.msgAlert('error', e.data, 3)
            })
    } else {
        indexStore.msgAlert('error', t('config.updatePlayoutFailed'), 2)
    }
}
</script>
<template>
    <div class="max-w-300 xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('config.playoutConf') }}</h2>
        <form
            v-if="configStore.playout"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitPlayout"
        >
            <div class="text-xl pt-3 md:text-right">{{ t('config.general') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.generalHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Stop Threshold</legend>
                    <input
                        v-model="configStore.playout.general.stop_threshold"
                        type="number"
                        min="3"
                        class="input input-sm w-full max-w-36"
                    />
                    <p class="fieldset-label items-baseline">
                        {{ t('config.stopThreshold') }}
                    </p>
                </fieldset>
            </div>

            <template v-if="configStore.playout.mail.show">
                <div class="text-xl pt-3 md:text-right">{{ t('config.mail') }}:</div>
                <div class="md:pt-4">
                    <label class="form-control mb-2">
                        <div class="whitespace-pre-line">
                            {{ t('config.mailHelp') }}
                        </div>
                    </label>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Subject</legend>
                        <input
                            v-model="configStore.playout.mail.subject"
                            type="text"
                            name="subject"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Recipient</legend>
                        <input
                            v-model="configStore.playout.mail.recipient"
                            type="text"
                            name="recipient"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Mail Level</legend>
                        <select v-model="configStore.playout.mail.mail_level" class="select select-sm w-full max-w-xs">
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Interval</legend>
                        <input
                            v-model="configStore.playout.mail.interval"
                            type="number"
                            min="30"
                            step="10"
                            class="input input-sm w-full max-w-36"
                        />
                        <p class="fieldset-label items-baseline">{{ t('config.mailInterval') }}</p>
                    </fieldset>
                </div>
            </template>

            <div class="text-xl pt-3 md:text-right">{{ t('config.logging') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.logHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">ffmpeg Level</legend>
                    <select v-model="configStore.playout.logging.ffmpeg_level" class="select select-sm w-full max-w-xs">
                        <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                    </select>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Ingest Level</legend>
                    <select v-model="configStore.playout.logging.ingest_level" class="select select-sm w-full max-w-xs">
                        <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                    </select>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.logging.detect_silence" type="checkbox" class="checkbox" />
                        Detect Silence
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.logDetect') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Ignore Lines</legend>
                    <input v-model="formatIgnoreLines" type="text" class="input input-sm w-full max-w-full truncate" />
                    <p class="fieldset-label items-baseline">{{ t('config.logIgnore') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.processing') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.processingHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Mode</legend>
                    <select v-model="configStore.playout.processing.mode" class="select select-sm w-full max-w-xs">
                        <option v-for="mode in processingMode" :key="mode" :value="mode">{{ mode }}</option>
                    </select>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.add_logo" type="checkbox" class="checkbox" />
                        Add Logo
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo</legend>
                    <input
                        v-model="configStore.playout.processing.logo"
                        type="text"
                        name="logo"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoPath') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Opacity</legend>
                    <input
                        v-model="configStore.playout.processing.logo_opacity"
                        type="number"
                        min="0"
                        max="1"
                        step="0.01"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Scale</legend>
                    <input
                        v-model="configStore.playout.processing.logo_scale"
                        type="text"
                        name="logo_scale"
                        class="input input-sm w-full max-w-md"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoScale') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Position</legend>
                    <input
                        v-model="configStore.playout.processing.logo_position"
                        type="text"
                        name="logo_position"
                        class="input input-sm w-full max-w-md"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoPosition') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Volumen</legend>
                    <div class="flex items-center gap-2">
                        <input
                            v-model.number="configStore.playout.processing.volume"
                            type="number"
                            min="0"
                            max="1.5"
                            step="0.001"
                            class="input input-sm w-36"
                        />
                    </div>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.vtt_enable" type="checkbox" class="checkbox" />
                        Enable VTT
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.processingVTTEnable') }}</p>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">VTT Dummy</legend>
                    <input
                        v-model="configStore.playout.processing.vtt_dummy"
                        type="text"
                        name="vtt_dummy"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingVTTDummy') }}</p>
                </fieldset>

                <div v-if="configStore.playout.processing.vtt_enable" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleName') }}</span>
                        <input
                            v-model.trim="configStore.playout.processing.vtt_name"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleLanguage') }}</span>
                        <input
                            v-model.trim="configStore.playout.processing.vtt_language"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleDefault') }}</span>
                        <input v-model="configStore.playout.processing.vtt_default" type="checkbox" class="toggle" />
                    </label>
                </div>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.ingest') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.ingestHelp') }}
                    </div>
                </label>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.ingest.enable" type="checkbox" class="checkbox" />
                        Enable
                    </label>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Input URL</legend>
                    <input
                        v-model="configStore.playout.ingest.ingest_url"
                        type="text"
                        class="input input-sm w-full max-w-lg"
                    />
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.playlist') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.playlistHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Day Start</legend>
                    <input
                        v-model="configStore.playout.playlist.day_start"
                        type="text"
                        name="day_start"
                        class="input input-sm w-full max-w-xs"
                        pattern="(([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]|now|none)"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.playlistDayStart') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Length</legend>
                    <input
                        v-model="configStore.playout.playlist.length"
                        type="text"
                        name="length"
                        class="input input-sm w-full max-w-xs"
                        pattern="([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.playlistLength') }}</p>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.playlist.infinit" type="checkbox" class="checkbox" />
                        Infinit
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.playlistInfinit') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.storage') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.storageHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Filler</legend>
                    <input
                        v-model="configStore.playout.storage.filler"
                        type="text"
                        name="filler"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.storageFiller') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Extensions</legend>
                    <input v-model="extensions" type="text" name="extensions" class="input input-sm w-full max-w-lg" />
                    <p class="fieldset-label items-baseline">{{ t('config.storageExtension') }}</p>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.storage.shuffle" type="checkbox" class="checkbox" />
                        Shuffle
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.storageShuffle') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.task') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.taskHelp') }}
                    </div>
                </label>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.task.enable" type="checkbox" class="checkbox" />
                        Enable
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Path</legend>
                    <input
                        v-model="configStore.playout.task.path"
                        type="text"
                        name="task_path"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.taskPath') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.output') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.outputHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Mode</legend>
                    <select v-model="output" class="select select-sm w-full max-w-xs">
                        <option v-for="output in configStore.outputs" :key="output.id" :value="output.name">
                            {{ output.name }}
                        </option>
                    </select>
                </fieldset>
                <div v-if="output === 'stream'" class="grid gap-3 sm:grid-cols-2">
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('config.streamType') }}</legend>
                        <select v-model="configStore.playout.output.stream_type" class="select select-sm w-full">
                            <option value="rtmp">RTMP</option>
                            <option value="srt">SRT</option>
                            <option value="udp">UDP</option>
                        </select>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('config.streamUrl') }}</legend>
                        <input
                            v-model="configStore.playout.output.stream_url"
                            type="url"
                            class="input input-sm w-full"
                        />
                    </fieldset>
                </div>
                <fieldset v-if="output === 'desktop'" class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input
                            v-model="configStore.playout.output.desktop_fullscreen"
                            type="checkbox"
                            class="checkbox"
                        />
                        Start Fullscreen
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.outputFormat') }}</legend>
                    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
                        <label class="fieldset">
                            <span class="fieldset-legend">Width</span>
                            <input
                                v-model.number="configStore.playout.output.width"
                                type="number"
                                min="1"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">Height</span>
                            <input
                                v-model.number="configStore.playout.output.height"
                                type="number"
                                min="1"
                                step="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">FPS</span>
                            <input
                                v-model.number="configStore.playout.output.fps"
                                type="number"
                                min="1"
                                step="0.01"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                </fieldset>

                <fieldset v-if="output === 'hls' || output === 'stream'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.encodingSettings') }}</legend>
                    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.videoCodec') }}</span>
                            <select v-model="configStore.playout.output.video_codec" class="select select-sm w-full">
                                <option v-for="codec in codecOptions.video" :key="codec.name" :value="codec.name">
                                    {{ codecLabel(codec) }}
                                </option>
                            </select>
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.audioCodec') }}</span>
                            <select v-model="configStore.playout.output.audio_codec" class="select select-sm w-full">
                                <option v-for="codec in codecOptions.audio" :key="codec.name" :value="codec.name">
                                    {{ codecLabel(codec) }}
                                </option>
                            </select>
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.videoPreset') }}</span>
                            <select v-model="configStore.playout.output.video_preset" class="select select-sm w-full">
                                <option v-for="preset in videoPresets" :key="preset" :value="preset">
                                    {{ preset }}
                                </option>
                            </select>
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.rateControl') }}</span>
                            <select v-model="configStore.playout.output.rate_control" class="select select-sm w-full">
                                <option value="crf">CRF</option>
                                <option value="cbr">CBR</option>
                            </select>
                        </label>
                        <label v-if="configStore.playout.output.rate_control === 'crf'" class="fieldset">
                            <span class="fieldset-legend">{{ t('config.videoQuality') }}</span>
                            <input
                                v-model.number="configStore.playout.output.video_quality"
                                type="number"
                                min="0"
                                max="51"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.videoMaxrate') }}</span>
                            <input
                                v-model.number="configStore.playout.output.video_maxrate"
                                type="number"
                                min="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.audioBitrate') }}</span>
                            <input
                                v-model.number="configStore.playout.output.audio_bitrate"
                                type="number"
                                min="1"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                </fieldset>

                <fieldset v-if="output === 'hls'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.hlsSettings') }}</legend>
                    <div class="grid gap-3 sm:grid-cols-3">
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.hlsPlaylistName') }}</span>
                            <input
                                v-model.trim="configStore.playout.output.hls_playlist_name"
                                type="text"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.hlsSegmentDuration') }}</span>
                            <input
                                v-model.number="configStore.playout.output.hls_segment_duration"
                                type="number"
                                min="1"
                                class="input input-sm w-full"
                            />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.hlsListSize') }}</span>
                            <input
                                v-model.number="configStore.playout.output.hls_list_size"
                                type="number"
                                min="0"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                    <p class="fieldset-label items-baseline">{{ t('config.outputParam') }}</p>
                </fieldset>

                <fieldset v-if="output === 'hls'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.hlsVariants') }}</legend>
                    <p class="fieldset-label items-baseline mb-2">{{ t('config.hlsVariantsHelp') }}</p>

                    <div
                        v-for="(variant, index) in hlsVariants"
                        :key="index"
                        class="flex flex-wrap items-center gap-2 mb-2"
                    >
                        <input
                            :value="variant.name"
                            @input="updateHlsVariant(index, 'name', ($event.target as HTMLInputElement).value)"
                            type="text"
                            placeholder="name"
                            class="input input-sm w-24"
                        />
                        <input
                            :value="variant.width"
                            @input="updateHlsVariant(index, 'width', ($event.target as HTMLInputElement).value)"
                            type="number"
                            placeholder="width"
                            class="input input-sm w-20"
                        />
                        <span>x</span>
                        <input
                            :value="variant.height"
                            @input="updateHlsVariant(index, 'height', ($event.target as HTMLInputElement).value)"
                            type="number"
                            placeholder="height"
                            class="input input-sm w-20"
                        />
                        <input
                            :value="variant.videoBitrate"
                            @input="updateHlsVariant(index, 'videoBitrate', ($event.target as HTMLInputElement).value)"
                            type="text"
                            placeholder="video bitrate, e.g. 5000k"
                            class="input input-sm w-32"
                        />
                        <input
                            :value="variant.audioBitrate"
                            @input="updateHlsVariant(index, 'audioBitrate', ($event.target as HTMLInputElement).value)"
                            type="text"
                            placeholder="audio bitrate, e.g. 128k"
                            class="input input-sm w-32"
                        />
                        <button type="button" class="btn btn-sm btn-error btn-outline" @click="removeHlsVariant(index)">
                            {{ t('config.remove') }}
                        </button>
                    </div>

                    <button type="button" class="btn btn-sm btn-outline mt-1" @click="addHlsVariant">
                        {{ t('config.addHlsVariant') }}
                    </button>
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
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>
