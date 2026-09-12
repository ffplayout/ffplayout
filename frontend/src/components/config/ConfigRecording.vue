<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const activeOutput = computed(() => configStore.outputs.find((output) => output.id === configStore.playout.output.id))

const hlsVariants = computed(() => {
    const output = configStore.playout.output
    if (output.mode !== 'hls' || output.id !== configStore.playout.recording.source_output_id) return []

    return [
        output.hls_playlist_name || 'stream',
        ...output.hls_variants.map((variant) => variant.split(':')[0]).filter(Boolean),
    ]
})

const copyOutputs = computed(() => {
    const mode = configStore.playout.recording.source === 'hls_variant' ? 'hls' : 'stream'
    return activeOutput.value?.name === mode ? [activeOutput.value] : []
})

const videoCodecs = computed(() => configStore.outputCodecs.recording.video)
const audioCodecs = computed(() => configStore.outputCodecs.recording.audio)
const newAudioOptionName = ref('')
const newAudioOptionValue = ref('')
const videoSettings = computed(
    () => videoCodecs.value.find((codec) => codec.name === configStore.playout.recording.video_codec)?.settings ?? [],
)
const audioUsesBitrate = computed(
    () =>
        audioCodecs.value.find((codec) => codec.name === configStore.playout.recording.audio_codec)?.uses_bitrate ??
        true,
)
function setRecordingVideoCodec() {
    const codec = videoCodecs.value.find((codec) => codec.name === configStore.playout.recording.video_codec)
    if (!codec) return
    configStore.playout.recording.video_options = Object.fromEntries(
        codec.settings.map((setting) => [setting.key, setting.default]),
    )
}

function setRecordingAudioCodec() {
    configStore.playout.recording.audio_options = {}
}

function setRecordingSource() {
    const recording = configStore.playout.recording
    if (recording.source === 'encode') return

    recording.source_output_id = activeOutput.value?.id ?? null
    if (recording.source === 'hls_variant') {
        recording.variant = hlsVariants.value[0] ?? ''
    }
}

function settingIsVisible(setting: EncoderSetting) {
    const condition = setting.visible_when
    return !condition || configStore.playout.recording.video_options[condition.key] === condition.value
}

function setVideoOption(key: string, value: string | number) {
    configStore.playout.recording.video_options = {
        ...configStore.playout.recording.video_options,
        [key]: String(value),
    }
}

function setAudioOption(key: string, value: string | number) {
    configStore.playout.recording.audio_options = {
        ...configStore.playout.recording.audio_options,
        [key]: String(value),
    }
}

function addAudioOption() {
    const options = configStore.playout.recording.audio_options
    const key = newAudioOptionName.value.trim()
    const value = newAudioOptionValue.value.trim()
    if (!key || !value || key in options) return
    configStore.playout.recording.audio_options = { ...options, [key]: value }
    newAudioOptionName.value = ''
    newAudioOptionValue.value = ''
}

function renameAudioOption(previousKey: string, event: Event) {
    const input = event.target as HTMLInputElement
    const key = input.value.trim()
    const options = { ...configStore.playout.recording.audio_options }
    if (!key || (key !== previousKey && key in options)) {
        input.value = previousKey
        return
    }
    const value = options[previousKey]
    delete options[previousKey]
    options[key] = value
    configStore.playout.recording.audio_options = options
}

function removeAudioOption(key: string) {
    const options = { ...configStore.playout.recording.audio_options }
    delete options[key]
    configStore.playout.recording.audio_options = options
}

function eventValue(event: Event) {
    return (event.target as HTMLInputElement).value
}

async function saveRecording() {
    try {
        const result = await configStore.setPlayoutConfig(configStore.playout)

        const id = configStore.channels[configStore.i]?.id
        const status = await authFetch<string>(`/api/control/${id}/process`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        if (status === 'active' && result.requires_restart) {
            configStore.showRestartModal = true
        }

        await configStore.getPlayoutConfig()
        indexStore.msgAlert('success', t('config.recordingUpdated'), 2)
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    }
}
</script>

<template>
    <div class="max-w-300 xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('config.recording') }}</h2>
        <form v-if="configStore.playout.recording" class="mt-10 max-w-3xl" @submit.prevent="saveRecording">
            <fieldset class="fieldset rounded-box border border-base-300 p-4">
                <label class="fieldset-label text-base-content">
                    <input v-model="configStore.playout.recording.enable" type="checkbox" class="checkbox" />
                    {{ t('config.recordingEnable') }}
                </label>
            </fieldset>

            <div class="mt-5 grid gap-4 sm:grid-cols-2">
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingSource') }}</span>
                    <select
                        v-model="configStore.playout.recording.source"
                        class="select select-sm w-full"
                        @change="setRecordingSource"
                    >
                        <option value="stream" :disabled="configStore.playout.output.mode !== 'stream'">
                            {{ t('config.recordingCurrentStream') }}
                        </option>
                        <option value="hls_variant" :disabled="configStore.playout.output.mode !== 'hls'">
                            {{ t('config.recordingHlsVariant') }}
                        </option>
                        <option value="encode">{{ t('config.recordingEncode') }}</option>
                    </select>
                </label>
                <label v-if="configStore.playout.recording.source !== 'encode'" class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingSourceOutput') }}</span>
                    <select
                        v-model.number="configStore.playout.recording.source_output_id"
                        class="select select-sm w-full"
                    >
                        <option :value="null" disabled>{{ t('config.recordingSelectOutput') }}</option>
                        <option v-for="output in copyOutputs" :key="output.id" :value="output.id">
                            {{ output.name }}
                        </option>
                    </select>
                </label>
                <label v-if="configStore.playout.recording.source === 'hls_variant'" class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingHlsVariant') }}</span>
                    <select v-model="configStore.playout.recording.variant" class="select select-sm w-full">
                        <option v-for="variant in hlsVariants" :key="variant" :value="variant">{{ variant }}</option>
                    </select>
                </label>
                <template v-if="configStore.playout.recording.source === 'encode'">
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.videoCodec') }}</span>
                        <select
                            v-model="configStore.playout.recording.video_codec"
                            class="select select-sm w-full"
                            @change="setRecordingVideoCodec"
                        >
                            <option v-for="codec in videoCodecs" :key="codec.name" :value="codec.name">
                                {{ codec.display_name }}
                            </option>
                        </select>
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.audioCodec') }}</span>
                        <select
                            v-model="configStore.playout.recording.audio_codec"
                            class="select select-sm w-full"
                            @change="setRecordingAudioCodec"
                        >
                            <option v-for="codec in audioCodecs" :key="codec.name" :value="codec.name">
                                {{ codec.display_name }}
                            </option>
                        </select>
                    </label>
                    <template v-for="setting in videoSettings" :key="setting.key">
                        <label v-if="settingIsVisible(setting)" class="fieldset">
                            <span class="fieldset-legend">{{ setting.label }}</span>
                            <select
                                v-if="setting.kind === 'select'"
                                :value="configStore.playout.recording.video_options[setting.key]"
                                class="select select-sm w-full"
                                @change="setVideoOption(setting.key, eventValue($event))"
                            >
                                <option v-for="choice in setting.choices" :key="choice.value" :value="choice.value">
                                    {{ choice.label }}
                                </option>
                            </select>
                            <input
                                v-else
                                :value="configStore.playout.recording.video_options[setting.key]"
                                type="number"
                                :min="setting.minimum ?? undefined"
                                :max="setting.maximum ?? undefined"
                                step="1"
                                class="input input-sm w-full"
                                @input="setVideoOption(setting.key, eventValue($event))"
                            />
                        </label>
                    </template>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.recordingWidth') }}</span>
                        <input
                            v-model.number="configStore.playout.recording.width"
                            type="number"
                            min="0"
                            step="2"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.recordingHeight') }}</span>
                        <input
                            v-model.number="configStore.playout.recording.height"
                            type="number"
                            min="0"
                            step="2"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label v-if="audioUsesBitrate" class="fieldset">
                        <span class="fieldset-legend">{{ t('config.audioBitrate') }}</span>
                        <input
                            v-model.number="configStore.playout.recording.audio_bitrate"
                            type="number"
                            min="1"
                            step="1"
                            class="input input-sm w-full"
                        />
                    </label>
                    <fieldset class="fieldset sm:col-span-2">
                        <legend class="fieldset-legend">{{ t('config.audioEncoderOptions') }}</legend>
                        <p class="fieldset-label items-baseline mb-2">{{ t('config.audioEncoderOptionsHelp') }}</p>
                        <div
                            v-for="[key, value] in Object.entries(configStore.playout.recording.audio_options)"
                            :key="key"
                            class="flex flex-wrap items-center gap-2 mb-2"
                        >
                            <input
                                :value="key"
                                type="text"
                                class="input input-sm w-48"
                                @change="renameAudioOption(key, $event)"
                            />
                            <input
                                :value="value"
                                type="text"
                                class="input input-sm grow"
                                @input="setAudioOption(key, eventValue($event))"
                            />
                            <button class="btn btn-sm btn-ghost" type="button" @click="removeAudioOption(key)">
                                {{ t('config.remove') }}
                            </button>
                        </div>
                        <div class="flex flex-wrap items-center gap-2">
                            <input
                                v-model="newAudioOptionName"
                                type="text"
                                class="input input-sm w-48"
                                :placeholder="t('config.audioOptionName')"
                            />
                            <input
                                v-model="newAudioOptionValue"
                                type="text"
                                class="input input-sm grow"
                                :placeholder="t('config.audioOptionValue')"
                            />
                            <button
                                class="btn btn-sm"
                                type="button"
                                :disabled="
                                    !newAudioOptionName.trim() ||
                                    !newAudioOptionValue.trim() ||
                                    newAudioOptionName.trim() in configStore.playout.recording.audio_options
                                "
                                @click="addAudioOption"
                            >
                                {{ t('config.addAudioOption') }}
                            </button>
                        </div>
                    </fieldset>
                </template>
                <label class="fieldset sm:col-span-2">
                    <span class="fieldset-legend">{{ t('config.recordingPath') }}</span>
                    <input
                        v-model.trim="configStore.playout.recording.path"
                        type="text"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingSegmentDuration') }}</span>
                    <input
                        v-model.number="configStore.playout.recording.segment_duration"
                        type="number"
                        min="30"
                        max="3600"
                        step="1"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingRetention') }}</span>
                    <input
                        v-model.number="configStore.playout.recording.retention_days"
                        type="number"
                        min="0"
                        step="1"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.recordingMinimumFreeSpace') }}</span>
                    <input
                        v-model.number="configStore.playout.recording.minimum_free_space_gb"
                        type="number"
                        min="0"
                        step="1"
                        class="input input-sm w-full"
                    />
                </label>
            </div>
            <button class="btn btn-primary mt-6" type="submit">{{ t('config.save') }}</button>
        </form>
    </div>

    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>
