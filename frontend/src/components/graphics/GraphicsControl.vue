<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { authFetch } from '@/composables/authFetch'
import { browseChannelPaths } from '@/composables/fileBrowser'
import { newGraphicEvent, validateGraphicEvents, type GraphicEvent } from '@/composables/playlistGraphics'
import { useConfig } from '@/stores/config'
import PlaylistGraphicsEditor from './PlaylistGraphicsEditor.vue'
import GraphicLayerCard from './GraphicLayerCard.vue'
import { useManualGraphicsControl } from '@/composables/manualGraphics'

const { t } = useI18n()
const config = useConfig()
const channel = computed(() => config.channels[config.i]?.id)
const draft = ref<GraphicEvent[]>([newGraphicEvent('take', 0, 1)])
const valid = computed(() => validateGraphicEvents(draft.value).length === 0)
const adding = ref(false)
const name = ref('')
const { status, busy, error, refresh, command, reset } = useManualGraphicsControl(() => channel.value, authFetch)
const displayedLayers = computed(() => [...(status.value?.layers ?? [])].sort((a, b) => b.layer - a.layer))
let generation = 0

function addLayer() {
    const next = Math.max(0, ...(status.value?.layers.map((layer) => layer.layer) ?? [])) + 1
    draft.value = [newGraphicEvent('take', 0, next)]
    name.value = ''
    adding.value = true
}

async function save(action: 'save' | 'take') {
    const version = generation
    const submitted = draft.value
    const submittedName = name.value
    const succeeded = await command({ action, layer: submitted[0]?.layer, name: submittedName, settings: submitted[0] })
    if (succeeded && version === generation && draft.value === submitted && name.value === submittedName) {
        adding.value = false
    }
}

function load(path: string, initial: boolean, signal: AbortSignal) {
    return browseChannelPaths(channel.value!, path, initial, signal)
}
watch(
    channel,
    () => {
        generation++
        reset()
        draft.value = [newGraphicEvent('take', 0, 1)]
        adding.value = false
        name.value = ''
        void refresh()
    },
    { immediate: true },
)
const timer = setInterval(refresh, 1000)
onBeforeUnmount(() => {
    generation++
    clearInterval(timer)
    reset()
})
</script>

<template>
    <div class="w-full max-w-5xl mx-auto">
        <p class="mb-4">{{ t('graphicsPage.manualHelp') }}</p>
        <p v-if="error" role="alert" class="text-error my-2">{{ error }}</p>
        <p v-if="status && !status.running" class="text-warning mb-3">{{ t('graphicsPage.stopped') }}</p>
        <div class="flex flex-wrap gap-2 mb-4">
            <button class="btn btn-sm" :disabled="busy || !status || status.layers.length >= 8" @click="addLayer">
                {{ t('graphicsPage.addLayer') }}
            </button>
            <button
                class="btn btn-sm btn-outline btn-error"
                :disabled="busy || !status?.layers.length"
                @click="command({ action: 'clear_all' })"
            >
                {{ t('graphicsPage.clearAll') }}
            </button>
        </div>
        <details
            v-if="adding || status?.layers.length === 0"
            class="collapse collapse-arrow bg-base-200 border border-base-content/30 mb-4"
            open
        >
            <summary class="collapse-title font-medium">{{ t('graphicsPage.newLayer') }}</summary>
            <div class="collapse-content">
                <fieldset :disabled="busy" class="min-w-0">
                    <label class="fieldset"
                        ><span>{{ t('graphicsPage.layerName') }}</span
                        ><input v-model="name" maxlength="200" class="input input-sm w-full"
                    /></label>
                    <PlaylistGraphicsEditor
                        v-if="channel"
                        v-model="draft"
                        :channel="channel"
                        :load="load"
                        :canvas-override="status?.canvas"
                        :duration="31536000"
                        manual
                        embedded
                    />
                    <div class="flex flex-wrap gap-2 mt-3">
                        <button class="btn btn-sm btn-primary" :disabled="!valid || !status" @click="save('save')">
                            {{ t('graphicsPage.saveLayer') }}
                        </button>
                        <button
                            class="btn btn-sm btn-success"
                            :disabled="!valid || !status?.running"
                            @click="save('take')"
                        >
                            {{ busy ? t('fileBrowser.loading') : t('graphicsPage.take') }}
                        </button>
                    </div>
                </fieldset>
            </div>
        </details>
        <div v-if="status && channel" class="grid gap-3">
            <GraphicLayerCard
                v-for="layer in displayedLayers"
                :key="`${channel}-${layer.layer}`"
                :layer="layer"
                :channel="channel"
                :canvas="status.canvas"
                :running="status.running"
                :busy="busy"
                :load="load"
                @command="command"
            />
        </div>
    </div>
</template>
