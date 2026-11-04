<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useElementSize } from '@vueuse/core'
import { useI18n } from 'vue-i18n'
import {
    graphicDragPosition,
    graphicFitSize,
    graphicResizeFromCorner,
    graphicSnapPosition,
    graphicSelectionRect,
} from '@/composables/graphicsLayout'
import { createFilePreviewUrl } from '@/composables/fileAccess'

const props = defineProps<{
    channel: number
    source: string
    canvas: [number, number]
    position: [number, number]
    size?: [number, number]
    sourceSize?: [number, number]
    opacity?: number
}>()
const emit = defineEmits<{
    layout: [value: { position?: [number, number]; size?: [number, number] }]
}>()
const { t } = useI18n()
const surface = ref<HTMLElement>()
const { width: previewWidth, height: previewHeight } = useElementSize(surface)
const url = ref('')
const failed = ref(false)
const guides = ref<[boolean, boolean]>([false, false])
const dragging = ref<{
    pointer: number
    x: number
    y: number
    origin: [number, number]
    size: [number, number]
    aspect: [number, number]
    mode: 'move' | 'resize' | 'resize-start'
}>()
const isVideo = computed(() => !/\.(png|gif|jpe?g|webp|bmp|tiff?|avif|ico)$/i.test(props.source))
const dimensions = computed(() => props.size ?? props.sourceSize)
const aspect = computed(() => props.sourceSize ?? dimensions.value)
const canResize = computed(() => positiveSize(dimensions.value) && positiveSize(aspect.value))
const canFit = computed(() => !!props.source && positiveSize(aspect.value))
const style = computed(() => ({
    left: `${(props.position[0] / props.canvas[0]) * 100}%`,
    top: `${(props.position[1] / props.canvas[1]) * 100}%`,
    width: `${((dimensions.value?.[0] ?? 100) / props.canvas[0]) * 100}%`,
    height: `${((dimensions.value?.[1] ?? 100) / props.canvas[1]) * 100}%`,
}))
const selection = computed(
    () =>
        dimensions.value &&
        graphicSelectionRect(props.position, dimensions.value, props.canvas, [previewWidth.value, previewHeight.value]),
)
const selectionStyle = computed(() =>
    selection.value?.compact
        ? {
              left: `${selection.value.left}px`,
              top: `${selection.value.top}px`,
              width: `${selection.value.width}px`,
              height: `${selection.value.height}px`,
          }
        : style.value,
)
const resizeCorners = computed<('resize-start' | 'resize')[]>(() =>
    selection.value?.compact ? ['resize'] : ['resize-start', 'resize'],
)
const resizeStyle = computed(() =>
    selection.value?.compact
        ? {
              left: `${selection.value.left + selection.value.width}px`,
              top: `${selection.value.top + selection.value.height}px`,
          }
        : {
              left: `${((props.position[0] + (dimensions.value?.[0] ?? 0)) / props.canvas[0]) * 100}%`,
              top: `${((props.position[1] + (dimensions.value?.[1] ?? 0)) / props.canvas[1]) * 100}%`,
          },
)
watch(
    () => [props.channel, props.source] as const,
    async (_, __, onCleanup) => {
        let cancelled = false
        onCleanup(() => {
            cancelled = true
            endDrag()
        })
        url.value = ''
        failed.value = false
        if (!props.source) return
        try {
            const preview = await createFilePreviewUrl(props.channel, props.source)
            if (!cancelled) url.value = preview
        } catch {
            if (!cancelled) failed.value = true
        }
    },
    { immediate: true },
)
function positiveSize(value?: [number, number]): value is [number, number] {
    return !!value && value.every((dimension) => Number.isFinite(dimension) && dimension > 0)
}

function down(event: PointerEvent, mode: 'move' | 'resize' | 'resize-start' = 'move') {
    if (event.button !== 0 || !canResize.value) return
    dragging.value = {
        pointer: event.pointerId,
        x: event.clientX,
        y: event.clientY,
        origin: [...props.position],
        size: [...dimensions.value!],
        aspect: [...aspect.value!],
        mode,
    }
    surface.value?.setPointerCapture(event.pointerId)
    event.preventDefault()
}

function move(event: PointerEvent) {
    const drag = dragging.value
    const bounds = surface.value?.getBoundingClientRect()
    if (!drag || drag.pointer !== event.pointerId || !bounds?.width || !bounds.height) return
    const delta: [number, number] = [event.clientX - drag.x, event.clientY - drag.y]
    const preview: [number, number] = [bounds.width, bounds.height]
    if (drag.mode !== 'move') {
        resize(drag.origin, drag.size, delta, preview, drag.aspect, drag.mode === 'resize-start')
    } else {
        const snapped = graphicSnapPosition(
            graphicDragPosition(drag.origin, delta, props.canvas, preview),
            drag.size,
            props.canvas,
            preview,
        )
        guides.value = snapped.guides
        emit('layout', { position: snapped.position })
    }
}

function resize(
    position: [number, number],
    size: [number, number],
    delta: [number, number],
    preview: [number, number],
    source: [number, number],
    fromStart: boolean,
) {
    const result = graphicResizeFromCorner(position, size, delta, props.canvas, preview, source, fromStart)
    emit('layout', result)
}

function endDrag() {
    dragging.value = undefined
    guides.value = [false, false]
}

function key(event: KeyboardEvent, mode: 'move' | 'resize' | 'resize-start' = 'move') {
    const delta: Record<string, [number, number]> = {
        ArrowLeft: [-1, 0],
        ArrowRight: [1, 0],
        ArrowUp: [0, -1],
        ArrowDown: [0, 1],
    }
    const direction = delta[event.key]
    if (!direction) return
    event.preventDefault()
    const step = event.shiftKey ? 10 : 2
    if (mode !== 'move' && canResize.value) {
        const width = (direction[0] || direction[1]) * step
        resize(
            [...props.position],
            dimensions.value!,
            [width, (width * aspect.value![1]) / aspect.value![0]],
            props.canvas,
            aspect.value!,
            mode === 'resize-start',
        )
    } else if (mode === 'move') {
        emit('layout', { position: [props.position[0] + direction[0] * step, props.position[1] + direction[1] * step] })
    }
}

function fit() {
    if (!canFit.value) return
    const size = graphicFitSize(aspect.value!, props.canvas)
    const position = [0, 1].map((axis) => {
        const current = Number.isFinite(props.position[axis]) ? props.position[axis] : 0
        return Math.round(Math.max(0, Math.min(props.canvas[axis] - size[axis], current)) / 2) * 2
    }) as [number, number]
    emit('layout', { size, position })
}
</script>

<template>
    <div class="sm:col-span-2 min-w-0">
        <div class="flex flex-wrap items-center justify-between gap-2 mb-1">
            <p class="text-sm font-semibold">{{ t('graphicsCanvas.title') }} · {{ canvas[0] }} × {{ canvas[1] }}</p>
            <button type="button" class="btn btn-xs" :disabled="!canFit" @click="fit">
                {{ t('graphicsCanvas.fit') }}
            </button>
        </div>
        <div
            ref="surface"
            class="relative overflow-hidden bg-base-300 border border-base-content/30 rounded-sm touch-none"
            :style="{
                aspectRatio: `${canvas[0]} / ${canvas[1]}`,
                backgroundImage:
                    'conic-gradient(color-mix(in srgb, currentColor 8%, transparent) 25%, transparent 0 50%, color-mix(in srgb, currentColor 8%, transparent) 0 75%, transparent 0)',
                backgroundSize: '20px 20px',
            }"
            @pointermove="move"
            @pointerup="endDrag"
            @pointercancel="endDrag"
            @lostpointercapture="endDrag"
        >
            <div v-if="source && dimensions" class="absolute pointer-events-none" :style="style">
                <video
                    v-if="url && isVideo && !failed"
                    :src="url"
                    autoplay
                    muted
                    loop
                    playsinline
                    class="w-full h-full pointer-events-none"
                    :style="{ opacity: opacity ?? 1 }"
                    @error="failed = true"
                />
                <img
                    v-else-if="url && !failed"
                    :src="url"
                    alt=""
                    draggable="false"
                    class="w-full h-full pointer-events-none"
                    :style="{ opacity: opacity ?? 1 }"
                    @error="failed = true"
                />
                <span v-else class="text-xs">{{ t('graphicsCanvas.placeholder') }}</span>
            </div>
            <button
                v-if="source && dimensions"
                type="button"
                class="absolute border border-info cursor-move p-0 bg-transparent"
                :class="{ 'border-dashed': selection?.compact }"
                :style="selectionStyle"
                :aria-label="t('graphicsCanvas.move')"
                @pointerdown="down"
                @keydown="key"
            />
            <div
                v-if="guides[0]"
                class="absolute inset-y-0 left-1/2 border-l border-info pointer-events-none z-20"
                aria-hidden="true"
            />
            <div
                v-if="guides[1]"
                class="absolute inset-x-0 top-1/2 border-t border-info pointer-events-none z-20"
                aria-hidden="true"
            />
            <template v-if="source && canResize">
                <button
                    v-for="corner in resizeCorners"
                    :key="corner"
                    type="button"
                    class="absolute z-10 w-4 h-4 border border-info bg-base-100 cursor-nwse-resize flex items-center justify-center"
                    :class="{ '-translate-x-full -translate-y-full': corner === 'resize' }"
                    :style="corner === 'resize' ? resizeStyle : { left: style.left, top: style.top }"
                    :aria-label="t('graphicsCanvas.resize')"
                    :title="t('graphicsCanvas.resize')"
                    @pointerdown.stop="down($event, corner)"
                    @keydown="key($event, corner)"
                >
                    <i class="bi-arrows-angle-expand text-xs" aria-hidden="true" />
                </button>
            </template>
        </div>
        <p class="text-xs text-base-content/70 mt-1">{{ t('graphicsCanvas.help') }}</p>
        <p v-if="selection?.compact" class="text-xs text-base-content/70">{{ t('graphicsCanvas.compact') }}</p>
        <p v-if="failed" class="text-xs text-warning">{{ t('graphicsCanvas.unavailable') }}</p>
    </div>
</template>
