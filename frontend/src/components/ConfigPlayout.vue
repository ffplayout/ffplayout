<template>
    <div class="max-w-[1200px] xs:pe-8">
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
                        <input v-model="configStore.playout.processing.audio_only" type="checkbox" class="checkbox" />
                        Audio Only
                    </label>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.copy_audio" type="checkbox" class="checkbox" />
                        Copy Audio
                    </label>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.copy_video" type="checkbox" class="checkbox" />
                        Copy Video
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Width</legend>
                    <input
                        v-model="configStore.playout.processing.width"
                        type="number"
                        min="-1"
                        step="1"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Height</legend>
                    <input
                        v-model="configStore.playout.processing.height"
                        type="number"
                        min="-1"
                        step="1"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Aspect</legend>
                    <input
                        v-model="configStore.playout.processing.aspect"
                        type="number"
                        min="1"
                        step="0.001"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">FPS</legend>
                    <input
                        v-model="configStore.playout.processing.fps"
                        type="number"
                        min="1"
                        step="0.01"
                        class="input input-sm w-full max-w-36"
                    />
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
                    <legend class="fieldset-legend">Audio Tracks</legend>
                    <input
                        v-model="configStore.playout.processing.audio_tracks"
                        type="number"
                        min="1"
                        max="255"
                        step="1"
                        class="input input-sm w-full max-w-36"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingAudioTracks') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Track Index</legend>
                    <input
                        v-model="configStore.playout.processing.audio_track_index"
                        type="number"
                        min="-1"
                        max="255"
                        step="1"
                        class="input input-sm w-full max-w-36"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingAudioIndex') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Audio Channels</legend>
                    <input
                        v-model="configStore.playout.processing.audio_channels"
                        type="number"
                        min="1"
                        max="255"
                        step="1"
                        class="input input-sm w-full max-w-36"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingAudioChannels') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Volumen</legend>
                    <input
                        v-model="configStore.playout.processing.volume"
                        type="number"
                        min="0"
                        max="1"
                        step="0.001"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Custom Filter</legend>
                    <textarea v-model="configStore.playout.processing.custom_filter" class="textarea w-full" rows="3" />
                    <p class="fieldset-label items-baseline">{{ t('config.processingCustomFilter') }}</p>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input
                            v-model="configStore.playout.processing.override_filter"
                            type="checkbox"
                            class="checkbox"
                        />
                        Override custom Filter
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.processingOverrideFilter') }}</p>
                </fieldset>

                <div v-if="configStore.playout.processing.override_filter" class="w-full py-0">
                    <span class="text-sm select-text font-bold text-orange-500">
                        {{ t('config.processingOverrideFilter') }}
                    </span>
                </div>

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
                    <legend class="fieldset-legend">Input Param</legend>
                    <input
                        v-model="configStore.playout.ingest.input_param"
                        type="text"
                        class="input input-sm w-full max-w-lg"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Custom Filter</legend>
                    <textarea v-model="configStore.playout.ingest.custom_filter" class="textarea w-full" rows="3" />
                    <p class="fieldset-label items-baseline">{{ t('config.ingestCustomFilter') }}</p>
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

            <div class="text-xl pt-3 md:text-right">{{ t('config.text') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.textHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.text.add_text" type="checkbox" class="checkbox" />
                        Add Text
                    </label>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Font</legend>
                    <input
                        v-model="configStore.playout.text.font"
                        type="text"
                        name="font"
                        class="input input-sm w-full max-w-lg"
                    />
                    <div class="label">
                        <span class="text-sm select-text text-base-content/80">{{ t('config.textFont') }}</span>
                    </div>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.text.text_from_filename" type="checkbox" class="checkbox" />
                        Text from File
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.textFromFile') }}</p>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Style</legend>
                    <input
                        v-model="configStore.playout.text.style"
                        type="text"
                        name="style"
                        class="input input-sm w-full truncate"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.textStyle') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Regex</legend>
                    <input
                        v-model="configStore.playout.text.regex"
                        type="text"
                        name="regex"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.textRegex') }}</p>
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
                        <option v-for="output in configStore.outputs" :key="output.id" :value="output.name">{{ output.name }}</option>
                    </select>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Output Parameter</legend>
                    <textarea v-model="configStore.playout.output.output_param" class="textarea w-full" rows="6" />
                    <p class="fieldset-label items-baseline">{{ t('config.outputParam') }}</p>
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

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/GenericModal.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const logLevels = ['INFO', 'WARNING', 'ERROR']
const processingMode = ['folder', 'playlist']

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
        return configStore.outputs.find(o => o.id === configStore.playout.output.id)?.name
    },

    set(value: string) {
        const output = configStore.outputs.find(o => o.name === value)
        configStore.playout.output.output_param = output?.parameters ?? ''
        configStore.playout.output.id = output?.id ?? 0
    },
})

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

async function onSubmitPlayout() {
    const update = await configStore.setPlayoutConfig(configStore.playout)
    configStore.onetimeInfo = true

    if (update.status === 200) {
        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        const channel = configStore.channels[configStore.i].id

        await fetch(`/api/control/${channel}/process/`, {
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
            .then(async (response: any) => {
                if (response === 'active') {
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
