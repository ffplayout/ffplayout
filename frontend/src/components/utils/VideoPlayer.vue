<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, shallowRef, watch } from 'vue'
import type { HlsJsVideoElement } from '@videojs/html/media/hlsjs-video'
import '@videojs/html/video/player'
import '@videojs/html/video/neutral-skin'
import '@videojs/html/live-video/player'
import '@videojs/html/live-video/neutral-skin'
import '@videojs/html/media/hlsjs-video'

// Keep Vue's self-component resolution distinct from the <video-player> element.
defineOptions({ name: 'VideoPlayback' })

const props = withDefaults(defineProps<{ src: string; type?: string; live?: boolean }>(), { live: false })
const media = shallowRef<HTMLVideoElement | HlsJsVideoElement>()
const generation = shallowRef(0)
const hls = computed(() => /mpegurl/i.test(props.type ?? '') || /\.m3u8(?:[?#]|$)/i.test(props.src))
let retry: ReturnType<typeof setTimeout> | undefined
let disposed = false
let resumeAfterRetry = false

function restoreVolume() {
    const saved = localStorage.getItem('volume')
    const volume = saved === null ? undefined : Number(saved)
    if (media.value && volume !== undefined && Number.isFinite(volume)) {
        media.value.volume = Math.min(1, Math.max(0, volume))
    }
}

function saveVolume() {
    if (media.value) localStorage.setItem('volume', String(media.value.volume))
}

function retryStream() {
    // File previews keep their error UI; only live streams retry missing media.
    if (!props.live || retry !== undefined) return
    retry = setTimeout(() => {
        retry = undefined
        if (!disposed) {
            resumeAfterRetry = media.value?.paused === false
            generation.value++
        }
    }, 2000)
}

function resumeStream() {
    if (!resumeAfterRetry || !media.value) return
    resumeAfterRetry = false
    void media.value.play().catch(() => {
        // Browser autoplay policy may still require another explicit click.
    })
}

watch(media, restoreVolume, { flush: 'post' })
watch(
    () => props.src,
    () => {
        clearTimeout(retry)
        retry = undefined
        resumeAfterRetry = false
    },
)

onMounted(restoreVolume)
onBeforeUnmount(() => {
    disposed = true
    clearTimeout(retry)
    media.value?.pause()
    if (media.value instanceof HTMLVideoElement) {
        media.value.removeAttribute('src')
        media.value.load()
    }
    // Removing the custom element releases the HLS engine and its requests.
})
</script>
<template>
    <component :is="live ? 'live-video-player' : 'video-player'" class="block w-full">
        <component :is="live ? 'live-video-neutral-skin' : 'video-neutral-skin'" class="block w-full aspect-video">
            <hlsjs-video
                v-if="hls"
                :key="`${src}:${generation}`"
                ref="media"
                :src="src"
                :type="type"
                preload="auto"
                playsinline
                @volumechange="saveVolume"
                @error="retryStream"
                @canplay="resumeStream"
            />
            <video v-else :key="src" ref="media" :src="src" preload="auto" playsinline @volumechange="saveVolume" />
        </component>
    </component>
</template>
