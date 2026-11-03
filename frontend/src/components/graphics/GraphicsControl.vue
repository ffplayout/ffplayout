<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { authFetch } from '@/composables/authFetch'
import { browseChannelPaths } from '@/composables/fileBrowser'
import { newGraphicEvent, validateGraphicEvents, type GraphicEvent } from '@/composables/playlistGraphics'
import { useConfig } from '@/stores/config'
import PlaylistGraphicsEditor from './PlaylistGraphicsEditor.vue'

interface Status {
    revision: number
    running: boolean
    canvas: [number, number]
    layers: {
        layer: number
        visible: boolean
        active: boolean
        position: [number, number]
        opacity: number
        z_index: number
    }[]
}
const { t } = useI18n()
const config = useConfig()
const channel = computed(() => config.channels[config.i]?.id)
const draft = ref<GraphicEvent[]>([newGraphicEvent('take', 0, 1)])
const valid = computed(() => validateGraphicEvents(draft.value).length === 0)
const status = ref<Status>()
const busy = ref(false)
const error = ref('')
let generation = 0
let poll: AbortController | undefined
async function refresh() {
    if (!channel.value || busy.value || poll) return
    const controller = new AbortController()
    poll = controller
    const version = generation
    try {
        const result = await authFetch<Status>(`/api/graphics/${channel.value}`, { signal: controller.signal })
        if (generation === version && !controller.signal.aborted) status.value = result
    } catch (reason) {
        if (!controller.signal.aborted && generation === version) {
            status.value = undefined
            error.value = reason instanceof Error ? reason.message : String(reason)
        }
    } finally {
        if (poll === controller) poll = undefined
    }
}

async function command(data: object) {
    if (!status.value || busy.value) return
    poll?.abort()
    poll = undefined
    const version = generation
    const id = channel.value
    const revision = status.value.revision
    busy.value = true
    error.value = ''
    try {
        await authFetch(`/api/graphics/${id}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ...data, revision }),
        })
    } catch (reason) {
        if (version === generation) error.value = reason instanceof Error ? reason.message : String(reason)
    } finally {
        if (version === generation) {
            busy.value = false
            await refresh()
        }
    }
}

function load(path: string, initial: boolean, signal: AbortSignal) {
    return browseChannelPaths(channel.value!, path, initial, signal)
}
watch(
    channel,
    () => {
        generation++
        poll?.abort()
        poll = undefined
        busy.value = false
        status.value = undefined
        error.value = ''
        draft.value = [newGraphicEvent('take', 0, 1)]
        void refresh()
    },
    { immediate: true },
)
const timer = setInterval(refresh, 1000)
onBeforeUnmount(() => {
    generation++
    clearInterval(timer)
    poll?.abort()
})
</script>

<template>
    <div class="w-full max-w-5xl mx-auto">
        <p v-if="error" role="alert" class="text-error my-2">{{ error }}</p>
        <p v-if="status && !status.running" class="text-warning">{{ t('graphicsPage.stopped') }}</p>
        <fieldset :disabled="busy" class="min-w-0">
            <PlaylistGraphicsEditor
                v-if="channel"
                v-model="draft"
                :channel="channel"
                :load="load"
                :canvas-override="status?.canvas"
                :duration="31536000"
                manual
            />
            <button
                type="button"
                class="btn btn-sm btn-success my-3"
                :disabled="!valid || draft[0]?.action !== 'take' || !status?.running"
                @click="command(draft[0])"
            >
                {{ busy ? t('fileBrowser.loading') : t('graphicsPage.take') }}
            </button>
        </fieldset>
        <table class="table table-sm mt-3">
            <thead>
                <tr>
                    <th>{{ t('graphicsEditor.layer') }}</th>
                    <th>{{ t('graphicsEditor.position') }}</th>
                    <th>{{ t('graphicsEditor.visible') }}</th>
                    <th>{{ t('graphicsEditor.action') }}</th>
                </tr>
            </thead>
            <tbody>
                <tr v-for="layer in status?.layers ?? []" :key="layer.layer">
                    <td>{{ layer.layer }}</td>
                    <td>{{ layer.position.join(', ') }}</td>
                    <td>{{ layer.active ? '●' : '○' }}</td>
                    <td class="flex flex-wrap gap-1">
                        <button
                            class="btn btn-xs"
                            type="button"
                            :disabled="busy"
                            @click="command({ action: 'stop', layer: layer.layer })"
                        >
                            {{ t('graphicsPage.stop') }}
                        </button>
                        <button
                            class="btn btn-xs"
                            type="button"
                            :disabled="busy"
                            @click="command({ action: 'visibility', layer: layer.layer, visible: !layer.visible })"
                        >
                            {{ t('graphicsPage.toggle') }}
                        </button>
                        <button
                            class="btn btn-xs"
                            type="button"
                            :disabled="busy || !valid"
                            @click="
                                command({
                                    action: 'update',
                                    layer: layer.layer,
                                    position: draft[0]?.position,
                                    opacity: draft[0]?.opacity,
                                    z_index: draft[0]?.z_index,
                                })
                            "
                        >
                            {{ t('graphicsPage.update') }}
                        </button>
                        <button
                            class="btn btn-xs btn-error"
                            type="button"
                            :disabled="busy"
                            @click="command({ action: 'clear', layer: layer.layer })"
                        >
                            {{ t('graphicsPage.clear') }}
                        </button>
                    </td>
                </tr>
            </tbody>
        </table>
        <button
            class="btn btn-sm btn-outline btn-error mt-3"
            type="button"
            :disabled="busy || !status?.layers.length"
            @click="command({ action: 'clear_all' })"
        >
            {{ t('graphicsPage.clearAll') }}
        </button>
    </div>
</template>
