<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useGraphicMetadata } from '@/composables/graphicMetadata'
import GraphicSourceInfo from './GraphicSourceInfo.vue'
import { useI18n } from 'vue-i18n'
import GraphicPositionEditor from '@/components/graphics/GraphicPositionEditor.vue'
import { useConfig } from '@/stores/config'
import FileBrowserModal from '@/components/utils/FileBrowserModal.vue'
import type { BrowserLoader } from '@/composables/fileBrowser'
import { graphicActions, newGraphicEvent, type GraphicAction, type GraphicEvent } from '@/composables/playlistGraphics'
import { usePlaylistGraphicsValidation } from '@/composables/playlistGraphicsValidation'

const model = defineModel<unknown>()
const props = defineProps<{
    duration: number
    channel: number
    load: BrowserLoader
    manual?: boolean
    canvasOverride?: [number, number]
}>()
const emit = defineEmits<{ validity: [valid: boolean] }>()
const { t } = useI18n()
const configStore = useConfig()
const canvas = computed<[number, number]>(
    () =>
        props.canvasOverride ?? [
            configStore.playoutSaved.program?.width ?? configStore.playoutSaved.output.width,
            configStore.playoutSaved.program?.height ?? configStore.playoutSaved.output.height,
        ],
)
const { modified, events, issues, canSave } = usePlaylistGraphicsValidation(
    () => model.value,
    () => props.duration,
)
const selected = ref<number | undefined>(0)
const browsing = ref<number>()
const beyondEnd = computed(() => events.value?.some((event) => event.at >= props.duration))
const repeats = ['repeat', 'source_repeat'] as const
const transitions = ['fade_in', 'duration', 'fade_out', 'pause'] as const
const extensions = ['png', 'gif', 'mov', 'qt', 'mp4', 'm4v', 'mkv', 'webm', 'avi', 'ts', 'm2ts', 'mts']

const sourcePath = computed(() => {
    const event = selected.value === undefined ? undefined : events.value?.[selected.value]
    return event?.action === 'take' ? event.source?.trim() : undefined
})
const mediaInfo = useGraphicMetadata(
    () => props.channel,
    () => sourcePath.value,
)

watch(canSave, (valid) => emit('validity', valid), { immediate: true, flush: 'sync' })

function publish(value: GraphicEvent[]) {
    modified.value = true
    model.value = value
}

function update(index: number, patch: Partial<GraphicEvent>) {
    const next = events.value!.slice()
    next[index] = { ...next[index], ...patch }
    if ('duration' in patch) delete next[index].hold
    publish(next)
}

function optional(index: number, key: string, enabled: boolean, value: unknown) {
    const next = events.value!.map((event) => ({ ...event }))
    if (key === 'duration') delete next[index].hold
    if (enabled) next[index][key] = value
    else delete next[index][key]
    publish(next)
}

function totalDuration(event: GraphicEvent) {
    return (
        event.duration ?? (event.hold == null ? undefined : (event.fade_in ?? 0) + event.hold + (event.fade_out ?? 0))
    )
}

function number(event: Event): number {
    return (event.target as HTMLInputElement).valueAsNumber
}

function text(event: Event): string {
    return (event.target as HTMLInputElement).value
}

function checked(event: Event): boolean {
    return (event.target as HTMLInputElement).checked
}

function pair(index: number, key: 'size' | 'position', axis: number, value: number) {
    const tuple = [...(events.value![index][key] ?? [0, 0])] as [number, number]
    tuple[axis] = value
    update(index, { [key]: tuple })
}

function add() {
    const rows = events.value ?? []
    const layer = Math.max(0, ...rows.map((event) => event.layer ?? 0)) + 1
    publish([...rows, newGraphicEvent('take', rows.at(-1)?.at ?? 0, layer)])
    selected.value = rows.length
}

function remove(index: number) {
    browsing.value = undefined
    publish(events.value!.filter((_, i) => i !== index))
    selected.value = undefined
}

function move(index: number, offset: number) {
    const rows = events.value!.slice()
    ;[rows[index], rows[index + offset]] = [rows[index + offset], rows[index]]
    browsing.value = undefined
    publish(rows)
    selected.value = index + offset
}

function sort() {
    browsing.value = undefined
    publish(events.value!.slice().sort((a, b) => a.at - b.at))
    selected.value = undefined
}

function action(index: number, value: GraphicAction) {
    const rows = events.value!.slice()
    rows[index] = newGraphicEvent(value, rows[index].at, rows[index].layer ?? 1)
    publish(rows)
}

function source(path: string) {
    if (browsing.value !== undefined) update(browsing.value, { source: path })
}
</script>

<template>
    <section class="mt-4 border-t border-base-content/30 pt-3">
        <div class="flex items-center justify-between gap-2">
            <h4 class="font-bold">
                {{ t('graphicsEditor.title') }}
                <span class="badge badge-sm">{{ t('graphicsEditor.experimental') }}</span>
            </h4>
            <span v-if="events && !manual" class="text-sm">{{ events.length }}/256</span>
        </div>
        <p class="text-sm text-base-content/70 my-2">
            {{ t(manual ? 'graphicsPage.manualHelp' : 'graphicsEditor.help') }}
        </p>
        <p v-if="events === undefined" role="alert" class="text-warning text-sm">
            {{ t('graphicsEditor.unsupported') }}
        </p>
        <p v-if="beyondEnd" role="status" class="text-warning text-sm mb-2">{{ t('graphicsEditor.beyondEnd') }}</p>
        <ul v-if="issues.length" role="alert" class="text-error text-sm mb-2">
            <li v-for="(issue, index) in issues" :key="index">
                {{ issue.index >= 0 ? `${issue.index + 1}: ` : '' }}{{ t(`graphicsEditor.errors.${issue.code}`) }}
            </li>
        </ul>
        <p v-if="!manual && !modified && canSave && (issues.length || events === undefined)" class="text-sm my-2">
            {{ t('graphicsEditor.preserve') }}
        </p>
        <div v-for="(event, index) in events" :key="index" class="border border-base-content/30 rounded-sm mb-2">
            <div class="flex items-center gap-1 bg-base-200 p-2">
                <button
                    type="button"
                    class="text-start flex-1 min-w-0 text-sm font-semibold"
                    :aria-expanded="selected === index"
                    @click="selected = selected === index ? undefined : index"
                >
                    {{ index + 1 }} · {{ event.at }} s · {{ t(`graphicsEditor.actions.${event.action}`) }}
                    <span v-if="event.layer !== undefined"> · {{ t('graphicsEditor.layer') }} {{ event.layer }}</span>
                </button>
                <button
                    type="button"
                    class="btn btn-xs btn-square"
                    v-if="!manual"
                    :disabled="index === 0"
                    :aria-label="t('graphicsEditor.up')"
                    @click="move(index, -1)"
                >
                    <i class="bi-arrow-up" />
                </button>
                <button
                    type="button"
                    class="btn btn-xs btn-square"
                    v-if="!manual"
                    :disabled="index === events!.length - 1"
                    :aria-label="t('graphicsEditor.down')"
                    @click="move(index, 1)"
                >
                    <i class="bi-arrow-down" />
                </button>
                <button
                    type="button"
                    class="btn btn-xs btn-square"
                    v-if="!manual"
                    :aria-label="t('graphicsEditor.remove')"
                    @click="remove(index)"
                >
                    <i class="bi-trash" />
                </button>
            </div>
            <div v-if="selected === index" class="p-3 grid grid-cols-1 sm:grid-cols-2 gap-3">
                <label v-if="!manual" class="fieldset"
                    ><span>{{ t('graphicsEditor.at') }}</span
                    ><input
                        :value="event.at"
                        type="number"
                        min="0"
                        step="0.01"
                        class="input input-sm w-full"
                        @input="update(index, { at: number($event) })"
                /></label>
                <label v-if="!manual" class="fieldset"
                    ><span>{{ t('graphicsEditor.action') }}</span
                    ><select
                        :value="event.action"
                        class="select select-sm w-full"
                        @change="action(index, text($event) as GraphicAction)"
                    >
                        <option v-for="value in graphicActions" :key="value" :value="value">
                            {{ t(`graphicsEditor.actions.${value}`) }}
                        </option>
                    </select></label
                >
                <label v-if="event.action !== 'clear_all'" class="fieldset"
                    ><span>{{ t('graphicsEditor.layer') }}</span
                    ><input
                        :value="event.layer"
                        type="number"
                        min="0"
                        step="1"
                        class="input input-sm w-full"
                        @input="update(index, { layer: number($event) })"
                /></label>
                <label v-if="event.action === 'visibility'" class="flex items-center gap-2"
                    ><input
                        type="checkbox"
                        class="checkbox checkbox-sm"
                        :checked="event.visible"
                        @change="update(index, { visible: checked($event) })"
                    />{{ t('graphicsEditor.visible') }}</label
                >
                <label v-if="event.action === 'take'" class="fieldset sm:col-span-2">
                    <span>{{ t('graphicsEditor.source') }}</span>
                    <span class="join w-full"
                        ><input
                            :value="event.source"
                            type="text"
                            class="input input-sm join-item min-w-0 w-full"
                            @input="update(index, { source: text($event) })" /><button
                            type="button"
                            class="btn btn-sm join-item"
                            :aria-label="t('fileBrowser.title')"
                            @click="browsing = index"
                        >
                            <i class="bi-folder2-open" /></button
                    ></span>
                    <GraphicSourceInfo :info="mediaInfo" />
                </label>
                <GraphicPositionEditor
                    v-if="event.action === 'take' && event.source && canvas[0] > 0 && canvas[1] > 0"
                    :channel="channel"
                    :source="event.source"
                    :canvas="canvas"
                    :position="event.position ?? [0, 0]"
                    :size="event.size ?? undefined"
                    :source-size="
                        mediaInfo?.width && mediaInfo?.height ? [mediaInfo.width, mediaInfo.height] : undefined
                    "
                    :opacity="event.opacity ?? undefined"
                    @position="update(index, { position: $event })"
                />
                <template v-if="event.action === 'take' || event.action === 'update'">
                    <div class="fieldset sm:col-span-2">
                        <label v-if="event.action === 'update'" class="flex gap-2 items-center"
                            ><input
                                type="checkbox"
                                class="checkbox checkbox-sm"
                                :checked="event.position != null"
                                @change="optional(index, 'position', checked($event), [0, 0])"
                            />{{ t('graphicsEditor.position') }}</label
                        >
                        <span v-else>{{ t('graphicsEditor.position') }}</span>
                        <div v-if="event.action === 'take' || event.position != null" class="grid grid-cols-2 gap-2">
                            <label class="fieldset"
                                ><span>X (px)</span
                                ><input
                                    :value="event.position?.[0] ?? 0"
                                    type="number"
                                    step="1"
                                    class="input input-sm w-full"
                                    @input="pair(index, 'position', 0, number($event))"
                            /></label>
                            <label class="fieldset"
                                ><span>Y (px)</span
                                ><input
                                    :value="event.position?.[1] ?? 0"
                                    type="number"
                                    step="1"
                                    class="input input-sm w-full"
                                    @input="pair(index, 'position', 1, number($event))"
                            /></label>
                        </div>
                    </div>
                    <div v-for="field in ['opacity', 'z_index'] as const" :key="field" class="fieldset">
                        <label v-if="event.action === 'update'" class="flex gap-2 items-center"
                            ><input
                                type="checkbox"
                                class="checkbox checkbox-sm"
                                :checked="event[field] != null"
                                @change="optional(index, field, checked($event), field === 'opacity' ? 1 : 0)"
                            />{{ t(`graphicsEditor.${field}`) }}</label
                        >
                        <label v-if="event.action === 'take' || event[field] != null" class="fieldset">
                            <span v-if="event.action === 'take'">{{ t(`graphicsEditor.${field}`) }}</span>
                            <input
                                :value="event[field] ?? (field === 'opacity' ? 1 : 0)"
                                type="number"
                                :min="field === 'opacity' ? 0 : undefined"
                                :max="field === 'opacity' ? 1 : undefined"
                                :step="field === 'opacity' ? 0.05 : 1"
                                :aria-label="t(`graphicsEditor.${field}`)"
                                class="input input-sm w-full"
                                @input="update(index, { [field]: number($event) })"
                            />
                        </label>
                    </div>
                </template>
                <template v-if="event.action === 'take'">
                    <div class="fieldset sm:col-span-2">
                        <label class="flex items-center gap-2"
                            ><input
                                type="checkbox"
                                class="checkbox checkbox-sm"
                                :checked="event.size != null"
                                @change="optional(index, 'size', checked($event), [320, 180])"
                            />{{ t('graphicsEditor.size') }}</label
                        >
                        <div v-if="event.size" class="grid grid-cols-2 gap-2">
                            <label class="fieldset"
                                ><span>{{ t('graphicsEditor.width') }}</span
                                ><input
                                    :value="event.size[0]"
                                    type="number"
                                    min="2"
                                    max="8192"
                                    step="2"
                                    class="input input-sm w-full"
                                    @input="pair(index, 'size', 0, number($event))"
                            /></label>
                            <label class="fieldset"
                                ><span>{{ t('graphicsEditor.height') }}</span
                                ><input
                                    :value="event.size[1]"
                                    type="number"
                                    min="2"
                                    max="8192"
                                    step="2"
                                    class="input input-sm w-full"
                                    @input="pair(index, 'size', 1, number($event))"
                            /></label>
                        </div>
                    </div>
                    <div v-for="field in transitions" :key="field" class="fieldset">
                        <label v-if="field === 'duration'" class="flex items-center gap-2"
                            ><input
                                type="checkbox"
                                class="checkbox checkbox-sm"
                                :checked="totalDuration(event) != null"
                                @change="optional(index, 'duration', checked($event), 4)"
                            />{{ t('graphicsEditor.duration') }}</label
                        >
                        <label v-if="field !== 'duration' || totalDuration(event) != null" class="fieldset"
                            ><span v-if="field !== 'duration'">{{ t(`graphicsEditor.${field}`) }}</span
                            ><input
                                :value="field === 'duration' ? totalDuration(event) : (event[field] ?? 0)"
                                type="number"
                                min="0"
                                step="0.1"
                                :aria-label="t(`graphicsEditor.${field}`)"
                                class="input input-sm w-full"
                                @input="update(index, { [field]: number($event) })"
                        /></label>
                        <span v-if="field === 'duration' && totalDuration(event) == null" class="text-xs">{{
                            t(manual ? 'graphicsPage.untilStop' : 'graphicsEditor.untilStop')
                        }}</span>
                    </div>
                    <div v-for="field in repeats" :key="field" class="fieldset">
                        <label class="fieldset"
                            ><span>{{ t(`graphicsEditor.${field}`) }}</span
                            ><select
                                :value="
                                    (event[field] ?? (field === 'repeat' ? 1 : 'forever')) === 'forever'
                                        ? 'forever'
                                        : 'count'
                                "
                                class="select select-sm w-full"
                                @change="update(index, { [field]: text($event) === 'forever' ? 'forever' : 1 })"
                            >
                                <option value="count">{{ t('graphicsEditor.count') }}</option>
                                <option value="forever">{{ t('graphicsEditor.forever') }}</option>
                            </select></label
                        >
                        <input
                            v-if="(event[field] ?? (field === 'repeat' ? 1 : 'forever')) !== 'forever'"
                            :value="event[field] ?? 1"
                            type="number"
                            min="1"
                            step="1"
                            :aria-label="t(`graphicsEditor.${field}`)"
                            class="input input-sm w-full"
                            @input="update(index, { [field]: number($event) })"
                        />
                    </div>
                    <label class="fieldset sm:col-span-2"
                        ><span>{{ t('graphicsEditor.source_end') }}</span
                        ><select
                            :value="event.source_end ?? 'hide'"
                            class="select select-sm w-full"
                            @change="update(index, { source_end: text($event) as 'hide' | 'hold_last' })"
                        >
                            <option value="hide">{{ t('graphicsEditor.hide') }}</option>
                            <option value="hold_last">{{ t('graphicsEditor.holdLast') }}</option>
                        </select></label
                    >
                    <p class="text-xs text-base-content/70 sm:col-span-2">
                        {{ t(manual ? 'graphicsPage.repeatHelp' : 'graphicsEditor.repeatHelp') }}
                    </p>
                </template>
            </div>
        </div>
        <div class="flex flex-wrap gap-2 my-2">
            <button
                v-if="events && !manual"
                type="button"
                class="btn btn-sm"
                :disabled="events.length >= 256"
                @click="add"
            >
                <i class="bi-plus-lg" />{{ t('graphicsEditor.add') }}
            </button>
            <button v-if="!manual && events && events.length > 1" type="button" class="btn btn-sm" @click="sort">
                {{ t('graphicsEditor.sort') }}
            </button>
            <button v-if="!manual && model != null" type="button" class="btn btn-sm" @click="publish([])">
                {{ t('graphicsEditor.clear') }}
            </button>
        </div>
        <details v-if="model != null" class="text-sm mt-2">
            <summary class="cursor-pointer">JSON</summary>
            <pre class="overflow-auto max-h-60 bg-base-200 rounded p-2 mt-2 text-xs">{{
                JSON.stringify(model, null, 2)
            }}</pre>
        </details>
        <FileBrowserModal
            :show="browsing !== undefined"
            :initial-path="browsing === undefined ? '' : (events?.[browsing]?.source ?? '')"
            path-format="relative"
            :extensions="extensions"
            :load="load"
            @select="source"
            @close="browsing = undefined"
        />
    </section>
</template>
