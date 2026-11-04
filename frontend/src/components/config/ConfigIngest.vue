<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
const { t } = useI18n()
const configStore = useConfig()
const indexStore = useIndex()
const logLevels = ['INFO', 'WARNING', 'ERROR']
function eventValue(event: Event): string {
    return (event.target as HTMLInputElement).value
}
const newListenerOptionNames = ref<Record<number, string>>({})
const newListenerOptionValues = ref<Record<number, string>>({})
const newListenerDemuxerNames = ref<Record<number, string>>({})
const newListenerDemuxerValues = ref<Record<number, string>>({})

function addLiveListener() {
    const listeners = configStore.playout.ingest.listeners
    listeners.push({
        id: 0,
        priority: 0,
        idle_timeout_ms: 2000,
        auto_takeover: true,
        enabled: false,
        name: '',
        backend: 'srt',
        identifier: 'srt://127.0.0.1:9000',
        options: {},
        demuxer_options: {},
    })
}

function removeLiveListener(index: number) {
    configStore.playout.ingest.listeners.splice(index, 1)
    newListenerOptionNames.value = {}
    newListenerOptionValues.value = {}
    newListenerDemuxerNames.value = {}
    newListenerDemuxerValues.value = {}
}

function changeLiveBackend(index: number) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    listener.identifier = listener.backend === 'srt' ? 'srt://127.0.0.1:9000' : 'rtmp://127.0.0.1:1936/live/stream'
    listener.options = {}
    listener.demuxer_options = {}
}

function addListenerOption(index: number) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    const name = (newListenerOptionNames.value[index] ?? '').trim()
    const value = newListenerOptionValues.value[index] ?? ''
    if (!name || !value.trim() || name in listener.options) return

    listener.options = { ...listener.options, [name]: value }
    newListenerOptionNames.value[index] = ''
    newListenerOptionValues.value[index] = ''
}

function renameListenerOption(index: number, previousName: string, event: Event) {
    const listener = configStore.playout.ingest.listeners[index]
    const input = event.target as HTMLInputElement
    if (!listener) return

    const name = input.value.trim()
    if (!name || (name !== previousName && name in listener.options)) {
        input.value = previousName
        return
    }

    const options = { ...listener.options }
    const value = options[previousName]
    delete options[previousName]
    options[name] = value
    listener.options = options
}

function removeListenerOption(index: number, name: string) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    const options = { ...listener.options }
    delete options[name]
    listener.options = options
}

function addListenerDemuxerOption(index: number) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    const name = (newListenerDemuxerNames.value[index] ?? '').trim()
    const value = newListenerDemuxerValues.value[index] ?? ''
    if (!name || !value.trim() || name in listener.demuxer_options) return

    listener.demuxer_options = { ...listener.demuxer_options, [name]: value }
    newListenerDemuxerNames.value[index] = ''
    newListenerDemuxerValues.value[index] = ''
}

function renameListenerDemuxerOption(index: number, previousName: string, event: Event) {
    const listener = configStore.playout.ingest.listeners[index]
    const input = event.target as HTMLInputElement
    if (!listener) return

    const name = input.value.trim()
    if (!name || (name !== previousName && name in listener.demuxer_options)) {
        input.value = previousName
        return
    }

    const options = { ...listener.demuxer_options }
    const value = options[previousName]
    delete options[previousName]
    options[name] = value
    listener.demuxer_options = options
}

function removeListenerDemuxerOption(index: number, name: string) {
    const listener = configStore.playout.ingest.listeners[index]
    if (!listener) return

    const options = { ...listener.demuxer_options }
    delete options[name]
    listener.demuxer_options = options
}

async function onSubmitPlayout() {
    try {
        await configStore.setPlayoutConfig(configStore.playout, 'ingest')
        configStore.onetimeInfo = true

        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        await configStore.getPlayoutConfig()
        await configStore.getPlayoutOutputs()
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    }
}
</script>
<template>
    <div class="w-full max-w-4xl pb-10">
        <h2 class="pt-3 text-3xl">{{ t('config.ingest') }}</h2>
        <form class="mt-6" @submit.prevent="onSubmitPlayout">
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.ingestHelp') }}
                    </div>
                </label>

                <div
                    v-for="(listener, index) in configStore.playout.ingest.listeners"
                    :key="listener.id || `new-${index}`"
                    class="rounded-box border border-base-300 p-3 mb-3"
                >
                    <div class="flex items-center justify-between gap-3">
                        <label class="fieldset-label text-base-content">
                            <input v-model="listener.enabled" type="checkbox" class="checkbox" />
                            {{ t('config.liveListenerEnabled') }}
                        </label>
                        <button type="button" class="btn btn-ghost btn-sm" @click="removeLiveListener(index)">
                            {{ t('config.liveListenerRemove') }}
                        </button>
                    </div>
                    <label class="flex items-center gap-3 mb-3">
                        <input v-model="listener.auto_takeover" type="checkbox" class="toggle toggle-primary" />
                        <span>{{ t('config.ingestAutoTakeover') }}</span>
                    </label>
                    <p class="text-sm mb-4 text-base-content/70">{{ t('config.ingestAutoTakeoverHelp') }}</p>
                    <div class="grid gap-3 md:grid-cols-2">
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.liveListenerName') }}</span>
                            <input v-model="listener.name" type="text" class="input input-sm w-full" />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.liveListenerBackend') }}</span>
                            <select
                                v-model="listener.backend"
                                class="select select-sm w-full"
                                @change="changeLiveBackend(index)"
                            >
                                <option value="rtmp">RTMP</option>
                                <option value="srt">SRT</option>
                            </select>
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.liveListenerUrl') }}</span>
                            <input v-model="listener.identifier" type="text" class="input input-sm w-full" />
                        </label>
                        <label class="fieldset">
                            <span class="fieldset-legend">{{ t('config.liveListenerPriority') }}</span>
                            <input
                                v-model.number="listener.priority"
                                type="number"
                                min="0"
                                max="100"
                                class="input input-sm w-full"
                            />
                        </label>
                    </div>
                    <div class="collapse collapse-plus bg-base-100/40 border-2 border-base-100 mt-4">
                        <input
                            type="checkbox"
                            :checked="
                                Object.keys(listener.options).length !== 0 ||
                                Object.keys(listener.demuxer_options).length !== 0
                            "
                        />
                        <div class="collapse-title font-semibold">{{ t('player.advanced') }}</div>
                        <div class="collapse-content">
                            <label class="fieldset mt-2">
                                <span class="fieldset-legend">{{ t('config.liveListenerIdleTimeout') }}</span>
                                <input
                                    :value="listener.idle_timeout_ms / 1000"
                                    type="number"
                                    required
                                    min="0.001"
                                    max="4294967.295"
                                    step="0.001"
                                    class="input input-sm w-full"
                                    @input="listener.idle_timeout_ms = Math.round(Number(eventValue($event)) * 1000)"
                                />
                                <span class="fieldset-label">{{ t('config.liveListenerIdleTimeoutHelp') }}</span>
                            </label>
                            <fieldset class="fieldset mt-2">
                                <legend class="fieldset-legend">{{ t('config.liveListenerOptions') }}</legend>
                                <p class="fieldset-label mb-2">{{ t('config.liveListenerOptionsHelp') }}</p>
                                <div
                                    v-for="[name, value] in Object.entries(listener.options)"
                                    :key="name"
                                    class="flex flex-wrap items-center gap-2 mb-2"
                                >
                                    <input
                                        :value="name"
                                        type="text"
                                        class="input input-sm w-48"
                                        @change="renameListenerOption(index, name, $event)"
                                    />
                                    <input
                                        :value="value"
                                        type="text"
                                        class="input input-sm grow"
                                        @input="listener.options[name] = eventValue($event)"
                                    />
                                    <button
                                        type="button"
                                        class="btn btn-sm btn-ghost"
                                        @click="removeListenerOption(index, name)"
                                    >
                                        {{ t('config.remove') }}
                                    </button>
                                </div>
                                <div class="flex flex-wrap items-center gap-2">
                                    <input
                                        v-model="newListenerOptionNames[index]"
                                        type="text"
                                        class="input input-sm w-48"
                                        :placeholder="t('config.protocolOptionName')"
                                    />
                                    <input
                                        v-model="newListenerOptionValues[index]"
                                        type="text"
                                        class="input input-sm grow"
                                        :placeholder="t('config.protocolOptionValue')"
                                    />
                                    <button type="button" class="btn btn-sm" @click="addListenerOption(index)">
                                        {{ t('config.addProtocolOption') }}
                                    </button>
                                </div>
                            </fieldset>
                            <fieldset class="fieldset mt-2">
                                <legend class="fieldset-legend">{{ t('config.liveDemuxerOptions') }}</legend>
                                <p class="fieldset-label mb-2">{{ t('config.liveDemuxerOptionsHelp') }}</p>
                                <div
                                    v-for="[name, value] in Object.entries(listener.demuxer_options)"
                                    :key="name"
                                    class="flex flex-wrap items-center gap-2 mb-2"
                                >
                                    <input
                                        :value="name"
                                        type="text"
                                        class="input input-sm w-48"
                                        @change="renameListenerDemuxerOption(index, name, $event)"
                                    />
                                    <input
                                        :value="value"
                                        type="text"
                                        class="input input-sm grow"
                                        @input="listener.demuxer_options[name] = eventValue($event)"
                                    />
                                    <button
                                        type="button"
                                        class="btn btn-sm btn-ghost"
                                        @click="removeListenerDemuxerOption(index, name)"
                                    >
                                        {{ t('config.remove') }}
                                    </button>
                                </div>
                                <div class="flex flex-wrap items-center gap-2">
                                    <input
                                        v-model="newListenerDemuxerNames[index]"
                                        type="text"
                                        class="input input-sm w-48"
                                        :placeholder="t('config.protocolOptionName')"
                                    />
                                    <input
                                        v-model="newListenerDemuxerValues[index]"
                                        type="text"
                                        class="input input-sm grow"
                                        :placeholder="t('config.protocolOptionValue')"
                                    />
                                    <button type="button" class="btn btn-sm" @click="addListenerDemuxerOption(index)">
                                        {{ t('config.addDemuxerOption') }}
                                    </button>
                                </div>
                            </fieldset>
                        </div>
                    </div>
                </div>
                <button type="button" class="btn btn-sm btn-outline" @click="addLiveListener">
                    {{ t('config.liveListenerAdd') }}
                </button>
            </div>

            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.ingestLogLevel') }}</legend>
                <select v-model="configStore.playout.logging.ingest_level" class="select select-sm w-full max-w-xs">
                    <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                </select>
            </fieldset>
            <button class="btn btn-primary mt-5" type="submit">{{ t('config.save') }}</button>
        </form>
    </div>
</template>
