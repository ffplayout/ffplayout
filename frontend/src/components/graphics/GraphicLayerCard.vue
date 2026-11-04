<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'
import { useI18n } from 'vue-i18n'
import { validateGraphicEvents, type GraphicEvent } from '@/composables/playlistGraphics'
import type { BrowserLoader } from '@/composables/fileBrowser'
import type { ManualGraphicLayer } from '@/composables/manualGraphics'
import PlaylistGraphicsEditor from './PlaylistGraphicsEditor.vue'

const props = defineProps<{
    layer: ManualGraphicLayer
    channel: number
    canvas: [number, number]
    running: boolean
    busy: boolean
    load: BrowserLoader
}>()
const emit = defineEmits<{ command: [data: object] }>()
const { t } = useI18n()
const name = ref(props.layer.name)
const draft = ref<GraphicEvent[]>([cloneDeep(props.layer.settings)])
const baseline = ref(cloneDeep([props.layer.name, props.layer.settings]))
const dirty = computed(() => !isEqual([name.value, draft.value[0]], baseline.value))
const valid = computed(() => validateGraphicEvents(draft.value).length === 0)

watch(
    () => JSON.stringify([props.layer.name, props.layer.settings]),
    () => {
        if (!dirty.value) reset()
        baseline.value = cloneDeep([props.layer.name, props.layer.settings])
    },
)

function reset() {
    name.value = props.layer.name
    draft.value = [cloneDeep(props.layer.settings)]
    baseline.value = cloneDeep([props.layer.name, props.layer.settings])
}

function save(action: 'save' | 'take') {
    emit('command', { action, layer: props.layer.layer, name: name.value, settings: draft.value[0] })
}
</script>

<template>
    <details class="collapse collapse-arrow bg-base-200 border border-base-content/30">
        <summary class="collapse-title flex flex-wrap items-center gap-2 font-medium">
            <span class="min-w-0 break-words">{{
                layer.name || t('graphicsPage.unnamedLayer', { id: layer.layer })
            }}</span>
            <span class="badge badge-sm" :class="layer.active ? 'badge-success' : 'badge-ghost'">
                {{
                    t(
                        layer.running
                            ? layer.visible
                                ? 'graphicsPage.started'
                                : 'graphicsPage.hidden'
                            : 'graphicsPage.layerStopped',
                    )
                }}
            </span>
            <span v-if="dirty" class="text-xs text-warning">{{ t('graphicsPage.unsaved') }}</span>
            <span class="flex flex-wrap gap-2 ml-auto">
                <button
                    class="btn btn-xs"
                    :disabled="busy || (!layer.running && !running)"
                    @click.stop.prevent="
                        emit('command', { action: layer.running ? 'stop' : 'start', layer: layer.layer })
                    "
                >
                    {{ t(layer.running ? 'graphicsPage.stop' : 'graphicsPage.start') }}
                </button>
                <button
                    class="btn btn-xs"
                    :disabled="busy || !layer.running"
                    @click.stop.prevent="
                        emit('command', { action: 'visibility', layer: layer.layer, visible: !layer.visible })
                    "
                >
                    {{ t('graphicsPage.toggle') }}
                </button>
                <button
                    class="btn btn-xs btn-error"
                    :disabled="busy"
                    @click.stop.prevent="emit('command', { action: 'clear', layer: layer.layer })"
                >
                    {{ t('graphicsPage.clear') }}
                </button>
            </span>
        </summary>
        <div class="collapse-content">
            <p v-if="layer.error" role="alert" class="text-error mb-2">{{ layer.error }}</p>
            <fieldset :disabled="busy" class="min-w-0">
                <label class="fieldset">
                    <span>{{ t('graphicsPage.layerName') }}</span>
                    <input v-model="name" maxlength="200" class="input input-sm w-full" />
                </label>
                <PlaylistGraphicsEditor
                    v-model="draft"
                    :channel="channel"
                    :canvas-override="canvas"
                    :load="load"
                    :duration="31536000"
                    manual
                    embedded
                    fixed-layer
                />
                <p class="text-xs text-base-content/70 mb-3">{{ t('graphicsPage.saveHelp') }}</p>
                <div class="flex flex-wrap gap-2">
                    <button class="btn btn-sm btn-primary" :disabled="!valid" @click="save('save')">
                        {{ t('graphicsPage.saveLayer') }}
                    </button>
                    <button class="btn btn-sm btn-success" :disabled="!valid || !running" @click="save('take')">
                        {{ t('graphicsPage.take') }}
                    </button>
                    <button
                        class="btn btn-sm"
                        :disabled="!valid"
                        @click="
                            emit('command', {
                                action: 'update',
                                layer: layer.layer,
                                position: draft[0]?.position,
                                opacity: draft[0]?.opacity,
                                z_index: draft[0]?.z_index,
                                fade_in: draft[0]?.fade_in ?? 0,
                                fade_out: draft[0]?.fade_out ?? 0,
                            })
                        "
                    >
                        {{ t('graphicsPage.update') }}
                    </button>
                    <button class="btn btn-sm btn-ghost" :disabled="!dirty" @click="reset">
                        {{ t('graphicsPage.resetEdits') }}
                    </button>
                </div>
            </fieldset>
        </div>
    </details>
</template>
