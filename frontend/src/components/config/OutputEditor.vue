<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useOutputLabels } from '@/composables/useOutputLabels'
import { useConfig } from '@/stores/config'
import { encodingOwner, outputVariantNames } from '@/composables/outputEncoding'
const props = defineProps<{ item: PlayoutConfigExt['output'] }>()
const editedOutput = computed(() => props.item)
const output = computed(() => props.item.mode)
const usesEncodingSource = computed(() => props.item.encoding_source_id !== null)
const allOutputs = computed(() => [configStore.playout.output, ...configStore.playout.outputs])
const encodingSources = computed(() =>
    allOutputs.value.filter((candidate) => {
        if (candidate.id <= 0) return false
        const owner = encodingOwner(candidate, allOutputs.value, props.item)
        if (!owner) return false
        return (
            codecOptions.value.video.some((codec) => codec.name === owner.video_codec) &&
            codecOptions.value.audio.some((codec) => codec.name === owner.audio_codec)
        )
    }),
)
const selectedSource = computed(() => encodingSources.value.find((item) => item.id === props.item.encoding_source_id))
const sourceVariants = computed(() => (selectedSource.value ? outputVariantNames(selectedSource.value) : []))
function changeEncodingSource() {
    editedOutput.value.encoding_variant = sourceVariants.value[0] ?? ''
    editedOutput.value.share_encoding = usesEncodingSource.value
}
const configStore = useConfig()
const { t } = useI18n()
const { outputModeLabel, encoderSettingLabel, encoderChoiceLabel } = useOutputLabels()
const newProtocolOptionName = ref('')
const newProtocolOptionValue = ref('')
const newMuxerOptionName = ref('')
const newMuxerOptionValue = ref('')
const newMetadataKey = ref('')
const newMetadataValue = ref('')
const newAudioOptionName = ref('')
const newAudioOptionValue = ref('')
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
        return editedOutput.value.hls_variants.map(parseHlsVariant)
    },

    set(rows: HlsVariantRow[]) {
        editedOutput.value.hls_variants = rows.map(serializeHlsVariant)
    },
})

const codecOptions = computed<OutputCodecOptions>(() => {
    if (output.value === 'stream') {
        return configStore.outputCodecs[editedOutput.value.stream_type ?? 'rtmp']
    }

    if (output.value === 'moq') return configStore.outputCodecs.moq ?? { video: [], audio: [] }
    return configStore.outputCodecs.hls
})

function codecLabel(codec: CodecOption): string {
    const label = codec.display_name || codec.name
    const details = codec.hardware ? `${codec.codec_id}, hardware` : codec.codec_id

    return `${codec.name} - ${label} (${details})`
}

const videoSettings = computed(
    () => codecOptions.value.video.find((codec) => codec.name === editedOutput.value.video_codec)?.settings ?? [],
)
const audioUsesBitrate = computed(
    () => codecOptions.value.audio.find((codec) => codec.name === editedOutput.value.audio_codec)?.uses_bitrate ?? true,
)

function settingIsVisible(setting: EncoderSetting): boolean {
    const condition = setting.visible_when
    return !condition || editedOutput.value.video_options[condition.key] === condition.value
}

function normalizeVideoOptions() {
    const options: Record<string, string> = {}
    for (const setting of videoSettings.value) {
        options[setting.key] = editedOutput.value.video_options?.[setting.key] ?? setting.default
    }
    editedOutput.value.video_options = options
}

function setVideoOption(key: string, value: string | number) {
    editedOutput.value.video_options = {
        ...editedOutput.value.video_options,
        [key]: String(value),
    }
}

function setAudioCodec() {
    editedOutput.value.audio_options = {}
}

function setAudioOption(key: string, value: string | number) {
    editedOutput.value.audio_options = {
        ...editedOutput.value.audio_options,
        [key]: String(value),
    }
}

function addAudioOption() {
    const options = editedOutput.value.audio_options
    const key = newAudioOptionName.value.trim()
    const value = newAudioOptionValue.value.trim()
    if (!key || !value || key in options) return
    editedOutput.value.audio_options = { ...options, [key]: value }
    newAudioOptionName.value = ''
    newAudioOptionValue.value = ''
}

function renameAudioOption(previousKey: string, event: Event) {
    const input = event.target as HTMLInputElement
    const key = input.value.trim()
    const options = { ...editedOutput.value.audio_options }
    if (!key || (key !== previousKey && key in options)) {
        input.value = previousKey
        return
    }
    const value = options[previousKey]
    delete options[previousKey]
    options[key] = value
    editedOutput.value.audio_options = options
}

function removeAudioOption(key: string) {
    const options = { ...editedOutput.value.audio_options }
    delete options[key]
    editedOutput.value.audio_options = options
}

function addMuxerOption() {
    const options = editedOutput.value.muxer_options
    const key = newMuxerOptionName.value.trim()
    const value = newMuxerOptionValue.value.trim()
    if (!key || !value || key in options) {
        return
    }
    editedOutput.value.muxer_options = { ...options, [key]: value }
    newMuxerOptionName.value = ''
    newMuxerOptionValue.value = ''
}

function addProtocolOption() {
    const options = editedOutput.value.protocol_options
    const key = newProtocolOptionName.value.trim()
    const value = newProtocolOptionValue.value
    if (!key || !value.trim() || key in options) {
        return
    }
    editedOutput.value.protocol_options = { ...options, [key]: value }
    newProtocolOptionName.value = ''
    newProtocolOptionValue.value = ''
}

function renameProtocolOption(previousKey: string, event: Event) {
    const input = event.target as HTMLInputElement
    const key = input.value.trim()
    const options = { ...editedOutput.value.protocol_options }
    if (!key || (key !== previousKey && key in options)) {
        input.value = previousKey
        return
    }
    const value = options[previousKey]
    delete options[previousKey]
    options[key] = value
    editedOutput.value.protocol_options = options
}

function setProtocolOption(key: string, value: string) {
    editedOutput.value.protocol_options = {
        ...editedOutput.value.protocol_options,
        [key]: value,
    }
}

function removeProtocolOption(key: string) {
    const options = { ...editedOutput.value.protocol_options }
    delete options[key]
    editedOutput.value.protocol_options = options
}

function renameMuxerOption(previousKey: string, event: Event) {
    const input = event.target as HTMLInputElement
    const key = input.value.trim()
    const options = { ...editedOutput.value.muxer_options }
    if (!key || (key !== previousKey && key in options)) {
        input.value = previousKey
        return
    }
    const value = options[previousKey]
    delete options[previousKey]
    options[key] = value
    editedOutput.value.muxer_options = options
}

function setMuxerOption(key: string, value: string) {
    editedOutput.value.muxer_options = {
        ...editedOutput.value.muxer_options,
        [key]: value,
    }
}

function removeMuxerOption(key: string) {
    const options = { ...editedOutput.value.muxer_options }
    delete options[key]
    editedOutput.value.muxer_options = options
}

function addMetadata() {
    const metadata = editedOutput.value.metadata_options
    const key = newMetadataKey.value.trim()
    const value = newMetadataValue.value.trim()
    if (!key || !value || Object.hasOwn(metadata, key)) return
    editedOutput.value.metadata_options = { ...metadata, [key]: value }
    newMetadataKey.value = ''
    newMetadataValue.value = ''
}

function renameMetadata(previousKey: string, event: Event) {
    const input = event.target as HTMLInputElement
    const key = input.value.trim()
    const metadata = { ...editedOutput.value.metadata_options }
    if (!key || (key !== previousKey && Object.hasOwn(metadata, key))) {
        input.value = previousKey
        return
    }
    const value = metadata[previousKey]
    delete metadata[previousKey]
    metadata[key] = value
    editedOutput.value.metadata_options = metadata
}

function setMetadata(key: string, value: string) {
    editedOutput.value.metadata_options = {
        ...editedOutput.value.metadata_options,
        [key]: value,
    }
}

function removeMetadata(key: string) {
    const metadata = { ...editedOutput.value.metadata_options }
    delete metadata[key]
    editedOutput.value.metadata_options = metadata
}

function eventValue(event: Event): string {
    return (event.target as HTMLInputElement).value
}

watch(
    () => [
        output.value,
        editedOutput.value.stream_type,
        editedOutput.value.video_codec,
        codecOptions.value,
        usesEncodingSource.value,
    ],
    () => {
        if (usesEncodingSource.value) return
        const video = codecOptions.value.video
        const audio = codecOptions.value.audio

        if (video.length > 0 && !video.some((codec) => codec.name === editedOutput.value.video_codec)) {
            editedOutput.value.video_codec = video[0].name
        }
        if (audio.length > 0 && !audio.some((codec) => codec.name === editedOutput.value.audio_codec)) {
            editedOutput.value.audio_codec = audio[0].name
            setAudioCodec()
        }
        normalizeVideoOptions()
    },
    { immediate: true },
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
</script>
<template>
    <div>
        <div v-if="editedOutput.mode === 'stream'" class="grid gap-3 sm:grid-cols-2">
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.streamType') }}</legend>
                <select v-model="editedOutput.stream_type" class="select select-sm w-full">
                    <option value="rtmp">RTMP</option>
                    <option value="srt">SRT</option>
                    <option value="udp">UDP</option>
                    <option value="custom">{{ t('config.custom') }}</option>
                </select>
            </fieldset>
            <fieldset v-if="editedOutput.stream_type === 'custom'" class="fieldset">
                <legend class="fieldset-legend">{{ t('config.streamFormat') }}</legend>
                <input
                    v-model="editedOutput.stream_format"
                    type="text"
                    class="input input-sm w-full"
                    placeholder="decklink"
                />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">
                    {{ editedOutput.stream_type === 'custom' ? t('config.streamTarget') : t('config.streamUrl') }}
                </legend>
                <input
                    v-model="editedOutput.stream_url"
                    :type="editedOutput.stream_type === 'custom' ? 'text' : 'url'"
                    class="input input-sm w-full"
                />
            </fieldset>
        </div>
        <fieldset v-if="editedOutput.mode === 'desktop'" class="fieldset mt-2 rounded-box w-full">
            <label class="fieldset-label text-base-content">
                <input v-model="editedOutput.desktop_fullscreen" type="checkbox" class="checkbox" />
                {{ t('config.fullscreen') }}
            </label>
        </fieldset>

        <fieldset v-if="output !== 'desktop'" class="fieldset">
            <label class="fieldset">
                <span class="fieldset-legend">{{ t('config.encodingSource') }}</span>
                <select
                    v-model="editedOutput.encoding_source_id"
                    class="select select-sm w-full"
                    @change="changeEncodingSource"
                >
                    <option :value="null">{{ t('config.encodingOwn') }}</option>
                    <option
                        v-if="usesEncodingSource && !selectedSource"
                        :value="editedOutput.encoding_source_id"
                        disabled
                    >
                        {{ t('config.encodingSourceUnavailable') }}
                    </option>
                    <option v-for="source in encodingSources" :key="source.id" :value="source.id">
                        {{ source.label || `${outputModeLabel(source.mode)} #${source.id}` }}
                    </option>
                </select>
            </label>
            <label v-if="sourceVariants.length" class="fieldset">
                <span class="fieldset-legend">{{ t('config.hlsVariants') }}</span>
                <select v-model="editedOutput.encoding_variant" class="select select-sm w-full">
                    <option v-for="name in sourceVariants" :key="name" :value="name">{{ name }}</option>
                </select>
            </label>
            <p class="text-sm">{{ t('config.encodingSourceHelp') }}</p>
        </fieldset>
        <fieldset v-if="!usesEncodingSource" class="fieldset">
            <legend class="fieldset-legend">{{ t('config.outputFormat') }}</legend>
            <label class="fieldset-label text-base-content">
                <input v-model="editedOutput.inherit_resolution" type="checkbox" class="checkbox checkbox-sm" />
                {{ t('config.outputInheritResolution') }}</label
            >
            <div v-if="!editedOutput.inherit_resolution" class="grid gap-3 sm:grid-cols-2">
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.width') }}</span>
                    <input
                        v-model.number="editedOutput.width"
                        type="number"
                        min="1"
                        step="1"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.height') }}</span>
                    <input
                        v-model.number="editedOutput.height"
                        type="number"
                        min="1"
                        step="1"
                        class="input input-sm w-full"
                    />
                </label>
            </div>
        </fieldset>

        <fieldset
            v-if="!usesEncodingSource && (output === 'hls' || output === 'stream' || output === 'moq')"
            class="fieldset"
        >
            <legend class="fieldset-legend">{{ t('config.encodingSettings') }}</legend>
            <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.videoCodec') }}</span>
                    <select v-model="editedOutput.video_codec" class="select select-sm w-full">
                        <option v-for="codec in codecOptions.video" :key="codec.name" :value="codec.name">
                            {{ codecLabel(codec) }}
                        </option>
                    </select>
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.audioCodec') }}</span>
                    <select v-model="editedOutput.audio_codec" class="select select-sm w-full" @change="setAudioCodec">
                        <option v-for="codec in codecOptions.audio" :key="codec.name" :value="codec.name">
                            {{ codecLabel(codec) }}
                        </option>
                    </select>
                </label>
                <template v-for="setting in videoSettings" :key="setting.key">
                    <label v-if="settingIsVisible(setting)" class="fieldset">
                        <span class="fieldset-legend">{{ encoderSettingLabel(setting) }}</span>
                        <select
                            v-if="setting.kind === 'select'"
                            :value="editedOutput.video_options[setting.key]"
                            class="select select-sm w-full"
                            @change="setVideoOption(setting.key, eventValue($event))"
                        >
                            <option v-for="choice in setting.choices" :key="choice.value" :value="choice.value">
                                {{ encoderChoiceLabel(choice) }}
                            </option>
                        </select>
                        <input
                            v-else
                            :value="editedOutput.video_options[setting.key]"
                            type="number"
                            :min="setting.minimum ?? undefined"
                            :max="setting.maximum ?? undefined"
                            step="1"
                            class="input input-sm w-full"
                            @input="setVideoOption(setting.key, eventValue($event))"
                        />
                    </label>
                </template>
                <label v-if="audioUsesBitrate" class="fieldset">
                    <span class="fieldset-legend">{{ t('config.audioBitrate') }}</span>
                    <input
                        v-model.number="editedOutput.audio_bitrate"
                        type="number"
                        min="1"
                        class="input input-sm w-full"
                    />
                </label>
            </div>
        </fieldset>

        <div v-if="output !== 'desktop'" class="collapse collapse-plus bg-base-100/40 border-2 border-base-100 my-4">
            <input
                type="checkbox"
                :checked="
                    (!usesEncodingSource && Object.keys(editedOutput.audio_options).length !== 0) ||
                    Object.keys(editedOutput.muxer_options).length !== 0 ||
                    Object.keys(editedOutput.metadata_options).length !== 0 ||
                    Object.keys(editedOutput.protocol_options).length !== 0
                "
                :aria-label="t('player.advanced')"
            />
            <div class="collapse-title font-semibold">{{ t('player.advanced') }}</div>
            <div class="collapse-content">
                <fieldset
                    v-if="!usesEncodingSource && (output === 'hls' || output === 'stream' || output === 'moq')"
                    class="fieldset"
                >
                    <legend class="fieldset-legend">{{ t('config.audioEncoderOptions') }}</legend>
                    <p class="fieldset-label items-baseline mb-2">{{ t('config.audioEncoderOptionsHelp') }}</p>
                    <div
                        v-for="[key, value] in Object.entries(editedOutput.audio_options)"
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
                                newAudioOptionName.trim() in editedOutput.audio_options
                            "
                            @click="addAudioOption"
                        >
                            {{ t('config.addAudioOption') }}
                        </button>
                    </div>
                </fieldset>

                <fieldset v-if="output === 'hls' || output === 'stream' || output === 'moq'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.muxerOptions') }}</legend>
                    <p class="fieldset-label items-baseline mb-2">{{ t('config.muxerOptionsHelp') }}</p>
                    <div
                        v-for="[key, value] in Object.entries(editedOutput.muxer_options)"
                        :key="key"
                        class="flex flex-wrap items-center gap-2 mb-2"
                    >
                        <input
                            :value="key"
                            type="text"
                            name="muxer_option_key"
                            class="input input-sm w-48"
                            @change="renameMuxerOption(key, $event)"
                        />
                        <input
                            :value="value"
                            type="text"
                            name="muxer_option_value"
                            class="input input-sm grow"
                            @input="setMuxerOption(key, eventValue($event))"
                        />
                        <button class="btn btn-sm btn-ghost" type="button" @click="removeMuxerOption(key)">
                            {{ t('config.remove') }}
                        </button>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                        <input
                            v-model="newMuxerOptionName"
                            type="text"
                            class="input input-sm w-48"
                            :placeholder="t('config.muxerOptionName')"
                        />
                        <input
                            v-model="newMuxerOptionValue"
                            type="text"
                            class="input input-sm grow"
                            :placeholder="t('config.muxerOptionValue')"
                        />
                        <button
                            class="btn btn-sm"
                            type="button"
                            :disabled="
                                !newMuxerOptionName.trim() ||
                                !newMuxerOptionValue.trim() ||
                                newMuxerOptionName.trim() in editedOutput.muxer_options
                            "
                            @click="addMuxerOption"
                        >
                            {{ t('config.addMuxerOption') }}
                        </button>
                    </div>
                </fieldset>

                <fieldset v-if="output === 'hls' || output === 'stream' || output === 'moq'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.outputMetadata') }}</legend>
                    <p class="fieldset-label items-baseline mb-2">{{ t('config.outputMetadataHelp') }}</p>
                    <div
                        v-for="[key, value] in Object.entries(editedOutput.metadata_options)"
                        :key="key"
                        class="flex flex-wrap items-center gap-2 mb-2"
                    >
                        <input
                            :value="key"
                            type="text"
                            class="input input-sm w-48"
                            @change="renameMetadata(key, $event)"
                        />
                        <input
                            :value="value"
                            type="text"
                            class="input input-sm grow"
                            @input="setMetadata(key, eventValue($event))"
                        />
                        <button class="btn btn-sm btn-ghost" type="button" @click="removeMetadata(key)">
                            {{ t('config.remove') }}
                        </button>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                        <input
                            v-model="newMetadataKey"
                            type="text"
                            class="input input-sm w-48"
                            :placeholder="t('config.metadataKey')"
                        />
                        <input
                            v-model="newMetadataValue"
                            type="text"
                            class="input input-sm grow"
                            :placeholder="t('config.metadataValue')"
                        />
                        <button
                            class="btn btn-sm"
                            type="button"
                            :disabled="
                                !newMetadataKey.trim() ||
                                !newMetadataValue.trim() ||
                                Object.hasOwn(editedOutput.metadata_options, newMetadataKey.trim())
                            "
                            @click="addMetadata"
                        >
                            {{ t('config.addMetadata') }}
                        </button>
                    </div>
                </fieldset>

                <fieldset v-if="output === 'stream'" class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.protocolOptions') }}</legend>
                    <p class="fieldset-label items-baseline mb-2">{{ t('config.protocolOptionsHelp') }}</p>
                    <div
                        v-for="[key, value] in Object.entries(editedOutput.protocol_options)"
                        :key="key"
                        class="flex flex-wrap items-center gap-2 mb-2"
                    >
                        <input
                            :value="key"
                            type="text"
                            name="protocol_option_key"
                            class="input input-sm w-48"
                            @change="renameProtocolOption(key, $event)"
                        />
                        <input
                            :value="value"
                            type="text"
                            name="protocol_option_value"
                            class="input input-sm grow"
                            @input="setProtocolOption(key, eventValue($event))"
                        />
                        <button class="btn btn-sm btn-ghost" type="button" @click="removeProtocolOption(key)">
                            {{ t('config.remove') }}
                        </button>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">
                        <input
                            v-model="newProtocolOptionName"
                            type="text"
                            class="input input-sm w-48"
                            :placeholder="t('config.protocolOptionName')"
                        />
                        <input
                            v-model="newProtocolOptionValue"
                            type="text"
                            class="input input-sm grow"
                            :placeholder="t('config.protocolOptionValue')"
                        />
                        <button
                            class="btn btn-sm"
                            type="button"
                            :disabled="
                                !newProtocolOptionName.trim() ||
                                !newProtocolOptionValue.trim() ||
                                newProtocolOptionName.trim() in editedOutput.protocol_options
                            "
                            @click="addProtocolOption"
                        >
                            {{ t('config.addProtocolOption') }}
                        </button>
                    </div>
                </fieldset>
            </div>
        </div>

        <fieldset v-if="output === 'hls'" class="fieldset">
            <legend class="fieldset-legend">{{ t('config.hlsSettings') }}</legend>
            <div class="grid gap-3 sm:grid-cols-3">
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.hlsPlaylistName') }}</span>
                    <input v-model.trim="editedOutput.hls_playlist_name" type="text" class="input input-sm w-full" />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.hlsSegmentDuration') }}</span>
                    <input
                        v-model.number="editedOutput.hls_segment_duration"
                        type="number"
                        min="1"
                        class="input input-sm w-full"
                    />
                </label>
                <label class="fieldset">
                    <span class="fieldset-legend">{{ t('config.hlsListSize') }}</span>
                    <input
                        v-model.number="editedOutput.hls_list_size"
                        type="number"
                        min="0"
                        class="input input-sm w-full"
                    />
                </label>
            </div>
            <p class="fieldset-label items-baseline">{{ t('config.outputParam') }}</p>
        </fieldset>

        <fieldset v-if="output === 'hls' && !usesEncodingSource" class="fieldset">
            <legend class="fieldset-legend">{{ t('config.hlsVariants') }}</legend>
            <p class="fieldset-label items-baseline mb-2">{{ t('config.hlsVariantsHelp') }}</p>

            <div v-for="(variant, index) in hlsVariants" :key="index" class="flex flex-wrap items-center gap-2 mb-2">
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
</template>
