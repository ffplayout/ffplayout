<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { graphicDragPosition } from '@/composables/graphicsLayout'
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
const emit = defineEmits<{ position: [value: [number, number]] }>()
const { t } = useI18n()
const surface = ref<HTMLElement>()
const url = ref('')
const failed = ref(false)
const dragging = ref<{ pointer: number; x: number; y: number; origin: [number, number] }>()
const isVideo = computed(() => !/\.(png|gif|jpe?g|webp|bmp|tiff?|avif|ico)$/i.test(props.source))
const dimensions = computed(() => props.size ?? props.sourceSize)
const style = computed(() => ({
    left: `${(props.position[0] / props.canvas[0]) * 100}%`,
    top: `${(props.position[1] / props.canvas[1]) * 100}%`,
    width: `${((dimensions.value?.[0] ?? 100) / props.canvas[0]) * 100}%`,
    height: `${((dimensions.value?.[1] ?? 100) / props.canvas[1]) * 100}%`,
}))
watch(
    () => [props.channel, props.source] as const,
    async (_, __, onCleanup) => {
        let cancelled = false
        onCleanup(() => {
            cancelled = true
            dragging.value = undefined
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
function down(event: PointerEvent) {
    if (event.button !== 0 || !dimensions.value) return
    dragging.value = { pointer: event.pointerId, x: event.clientX, y: event.clientY, origin: [...props.position] }
    surface.value?.setPointerCapture(event.pointerId)
    event.preventDefault()
}

function move(event: PointerEvent) {
    const drag = dragging.value
    const bounds = surface.value?.getBoundingClientRect()
    if (!drag || drag.pointer !== event.pointerId || !bounds?.width || !bounds.height) return
    emit(
        'position',
        graphicDragPosition(drag.origin, [event.clientX - drag.x, event.clientY - drag.y], props.canvas, [
            bounds.width,
            bounds.height,
        ]),
    )
}

function key(event: KeyboardEvent) {
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
    emit('position', [props.position[0] + direction[0] * step, props.position[1] + direction[1] * step])
}
</script>

<template>
    <div class="sm:col-span-2 min-w-0">
        <p class="text-sm font-semibold mb-1">{{ t('graphicsCanvas.title') }} · {{ canvas[0] }} × {{ canvas[1] }}</p>
        <div
            ref="surface"
            class="relative overflow-hidden bg-base-300 border border-base-content/30 rounded-sm touch-none"
            :style="{ aspectRatio: `${canvas[0]} / ${canvas[1]}` }"
            @pointermove="move"
            @pointerup="dragging = undefined"
            @pointercancel="dragging = undefined"
            @lostpointercapture="dragging = undefined"
        >
            <button
                v-if="source && dimensions"
                type="button"
                class="absolute border border-info cursor-move p-0 bg-transparent"
                :style="style"
                :aria-label="t('graphicsCanvas.move')"
                @pointerdown="down"
                @keydown="key"
            >
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
            </button>
        </div>
        <p class="text-xs text-base-content/70 mt-1">{{ t('graphicsCanvas.help') }}</p>
        <p v-if="failed" class="text-xs text-warning">{{ t('graphicsCanvas.unavailable') }}</p>
    </div>
</template>
