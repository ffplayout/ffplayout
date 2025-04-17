<template>
    <div>
        <div v-if="options.sources">
            <video
                :id="reference"
                class="video-js vjs-default-skin vjs-big-play-centered vjs-16-9"
                width="1024"
                height="576"
            />
        </div>
    </div>
</template>

<script setup lang="ts">
import { ref, nextTick, onMounted, onBeforeUnmount } from 'vue'
import videojs from 'video.js'
import 'video.js/dist/video-js.css'

import { useConfig } from '@/stores/config'

const configStore = useConfig()

const player = ref()

const props = defineProps({
    options: {
        type: Object,
        required: true,
    },
    reference: {
        type: String,
        required: true,
    },
})

onMounted(() => {
    const volume = localStorage.getItem('volume')

    player.value = videojs(props.reference, props.options, function onPlayerReady() {
        // console.log('onPlayerReady', this);
    })

    if (volume !== null) {
        player.value.volume(volume)
    }

    player.value.on('volumechange', () => {
        localStorage.setItem('volume', player.value.volume())
    })

    player.value.on('error', () => {
        setTimeout(() => {
            configStore.showPlayer = false

            nextTick(() => {
                configStore.showPlayer = true
            })
        }, 2000)
    })
})

onBeforeUnmount(() => {
    if (player.value) {
        player.value.dispose()
    }
})
</script>

<style>
.video-js .vjs-volume-panel.vjs-volume-panel-horizontal {
    width: 10em;
}

.video-js .vjs-volume-panel .vjs-volume-control.vjs-volume-horizontal {
    width: 5em;
    height: 3em;
    margin-right: 0;
    opacity: 1;
}
</style>
