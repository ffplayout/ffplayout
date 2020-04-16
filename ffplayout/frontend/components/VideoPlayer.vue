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

<script>
/* eslint-disable camelcase */
import videojs from 'video.js'
require('video.js/dist/video-js.css')

export default {
    name: 'VideoPlayer',
    props: {
        options: {
            type: Object,
            default () {
                return {}
            }
        },
        reference: {
            type: String,
            default () {
                return ''
            }
        }
    },
    data () {
        return {
            player: null
        }
    },

    mounted () {
        this.player = videojs(this.reference, this.options, function onPlayerReady () {
            // console.log('onPlayerReady', this);
        })
    },

    beforeDestroy () {
        if (this.player) {
            this.player.dispose()
        }
    },

    methods: {
    }
}
</script>
