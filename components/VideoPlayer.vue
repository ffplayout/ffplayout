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
import videojs from 'video.js'
import 'video.js/dist/video-js.css'

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
    player.value = videojs(props.reference, props.options, function onPlayerReady() {
        // console.log('onPlayerReady', this);
    })
})

onBeforeUnmount(() => {
    if (player.value) {
        player.value.dispose()
    }
})
</script>
