<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useConfig } from '@/stores/config'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { authFetch } from '@/composables/authFetch'
import { useOutputLabels } from '@/composables/useOutputLabels'
import OutputEditor from './OutputEditor.vue'
import GenericModal from '@/components/utils/GenericModal.vue'
const configStore = useConfig()
const authStore = useAuth()
const indexStore = useIndex()
const { t } = useI18n()
const { outputModeLabel, outputStatusLabel } = useOutputLabels()
const instances = computed(() =>
    [configStore.playout.output, ...(configStore.playout.outputs ?? [])].sort(
        (a, b) => Number(b.mode === 'moq') - Number(a.mode === 'moq'),
    ),
)
const newMode = ref<'hls' | 'stream' | 'desktop' | 'moq'>('stream')
const keys = new WeakMap<object, number>()
let nextKey = 0
function instanceKey(item: object) {
    if (!keys.has(item)) keys.set(item, ++nextKey)
    return keys.get(item)!
}
function runtime(output: PlayoutConfigExt['output']) {
    return configStore.outputs.find((item) => item.id === output.id && item.name === output.mode)
}
function addOutput() {
    if (instances.value.length >= 8 || (newMode.value === 'desktop' && !configStore.outputCodecs.desktop_available))
        return
    const item = JSON.parse(JSON.stringify(configStore.playout.output)) as PlayoutConfigExt['output']
    Object.assign(item, {
        id: 0,
        label: '',
        enabled: true,
        inherit_resolution: true,
        share_encoding: true,
        encoding_source_id: null,
        encoding_variant: '',
        public_moq: false,
        mode: newMode.value,
        protocol_options: {},
        muxer_options: {},
        metadata_options: {},
        audio_options: {},
        hls_variants: [],
    })
    item.hls_playlist_name = `stream_${Date.now()}`
    if (item.mode === 'stream') {
        item.stream_type = 'rtmp'
        item.stream_url = 'rtmp://127.0.0.1/live/stream'
    }
    if (item.mode === 'moq') {
        item.video_codec = 'libx264'
        item.audio_codec = 'aac'
        item.video_options = {
            preset: 'veryfast',
            tune: 'zerolatency',
            rate_control: 'crf',
            quality: '23',
            maxrate: '2400',
        }
    }
    configStore.playout.outputs.push(item)
}
function removeOutput(item: PlayoutConfigExt['output']) {
    const index = [configStore.playout.output, ...configStore.playout.outputs].indexOf(item)
    if (instances.value.length < 2) return
    if (index === 0) configStore.playout.output = configStore.playout.outputs.shift()!
    else configStore.playout.outputs.splice(index - 1, 1)
}
async function onSubmitPlayout() {
    try {
        const update = await configStore.setPlayoutConfig(configStore.playout)
        configStore.onetimeInfo = true
        const { requires_restart: requiresRestart } = update

        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        const id = configStore.channels[configStore.i]?.id
        const response = await authFetch<string>(`/api/control/${id}/process`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        if (response === 'active' && requiresRestart) {
            configStore.showRestartModal = true
        }

        await configStore.getPlayoutConfig()
        await configStore.getPlayoutOutputs()
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    }
}
let statusTimer: ReturnType<typeof setTimeout> | undefined
let disposed = false
const retrying = ref(new Set<number>())
async function retryOutput(item: PlayoutConfigExt['output']) {
    if (retrying.value.has(item.id)) return
    retrying.value.add(item.id)
    try {
        const channel = configStore.channels[configStore.i]?.id
        await authFetch(`/api/playout/outputs/${channel}/${item.id}/retry`, { method: 'POST' })
        await configStore.getPlayoutOutputs()
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    } finally {
        retrying.value.delete(item.id)
    }
}
async function refreshStatus() {
    try {
        await configStore.getPlayoutOutputs()
    } catch {
        // A temporary API failure must not turn the last known state into "stopped".
    } finally {
        if (!disposed) statusTimer = setTimeout(refreshStatus, 5000)
    }
}
onMounted(refreshStatus)
onUnmounted(() => {
    disposed = true
    clearTimeout(statusTimer)
})
</script>
<template>
    <div class="w-full max-w-4xl pb-10">
        <div class="flex flex-wrap items-center gap-3 pt-3">
            <h2 class="text-3xl grow">{{ t('config.outputs') }}</h2>
            <button class="btn btn-sm" type="button" @click="configStore.getPlayoutOutputs()">
                {{ t('config.outputRefresh') }}
            </button>
        </div>
        <p class="my-5">{{ t('config.outputHelp') }}</p>
        <p class="text-sm mb-5">{{ t('config.outputSharingHelp') }}</p>
        <form @submit.prevent="onSubmitPlayout">
            <div class="space-y-4">
                <section
                    v-for="item in instances"
                    :key="instanceKey(item)"
                    class="border border-base-300 rounded-box bg-base-100/40 p-4"
                >
                    <div class="flex flex-wrap items-center gap-3 mb-3">
                        <strong>{{ outputModeLabel(item.mode) }}</strong>
                        <input
                            v-model.trim="item.label"
                            :placeholder="t('config.outputName')"
                            :aria-label="t('config.outputName')"
                            maxlength="128"
                            class="input input-sm grow"
                        />
                        <label class="fieldset-label text-base-content"
                            ><input v-model="item.enabled" type="checkbox" class="checkbox checkbox-sm" />{{
                                t('config.enable')
                            }}</label
                        >
                        <button
                            type="button"
                            class="btn btn-sm btn-ghost"
                            :disabled="instances.length < 2"
                            @click="removeOutput(item)"
                        >
                            {{ t('config.outputRemove') }}
                        </button>
                    </div>
                    <p class="text-sm mb-3" role="status">
                        {{ t('config.outputRuntime') }}: {{ outputStatusLabel(runtime(item)?.runtime_status) }}
                        <span v-if="runtime(item)?.runtime_error" class="text-error">
                            — {{ runtime(item)?.runtime_error }}</span
                        >
                    </p>
                    <button
                        v-if="authStore.role !== 'user' && item.id > 0 && item.mode !== 'desktop'"
                        type="button"
                        class="btn btn-sm mb-3"
                        :disabled="
                            retrying.has(item.id) ||
                            !runtime(item)?.runtime_retryable ||
                            runtime(item)?.runtime_status !== 'failed'
                        "
                        @click="retryOutput(item)"
                    >
                        {{ t('config.outputRetry') }}
                    </button>
                    <p v-if="item.mode === 'moq'" class="text-sm mb-3">{{ t('config.moqPlayerPreviewHelp') }}</p>
                    <label v-if="item.mode === 'moq'" class="flex items-center gap-2 my-3">
                        <input v-model="item.public_moq" type="checkbox" class="checkbox checkbox-sm" />
                        {{ t('config.moqPublicPlayer') }}
                    </label>
                    <p v-if="item.mode === 'moq'" class="text-sm mb-3">{{ t('config.moqPublicPlayerHelp') }}</p>
                    <a
                        v-if="item.mode === 'moq' && item.public_moq"
                        :href="`/moq/player?channel=${configStore.channels[configStore.i]?.id}`"
                        target="_blank"
                        rel="noopener"
                        class="link inline-block mb-3"
                        >{{ t('config.previewOpen') }}</a
                    >
                    <OutputEditor :item="item" />
                </section>
            </div>
            <div class="flex flex-wrap gap-2 my-5">
                <select v-model="newMode" :aria-label="t('config.outputNewType')" class="select select-sm">
                    <option value="hls">{{ t('config.outputModes.hls') }}</option>
                    <option value="stream">{{ t('config.outputModes.stream') }}</option>
                    <option
                        v-if="configStore.outputCodecs.desktop_available"
                        value="desktop"
                        :disabled="instances.some((item) => item.mode === 'desktop')"
                    >
                        {{ t('config.outputModes.desktop') }}
                    </option>
                    <option value="moq" :disabled="instances.some((item) => item.mode === 'moq')">
                        {{ t('config.outputModes.moq') }}
                    </option>
                </select>
                <button
                    type="button"
                    class="btn btn-sm"
                    :disabled="
                        instances.length >= 8 ||
                        (['desktop', 'moq'].includes(newMode) && instances.some((item) => item.mode === newMode))
                    "
                    @click="addOutput"
                >
                    {{ t('config.outputAdd') }}
                </button>
            </div>
            <p class="text-sm my-4">{{ t('config.outputRestartHelp') }}</p>
            <button class="btn btn-primary" type="submit">{{ t('config.save') }}</button>
        </form>
    </div>
    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>
