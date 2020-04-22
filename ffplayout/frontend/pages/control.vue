<template>
    <div style="height:100%;">
        <Menu />
        <b-container class="control-container">
            <b-row class="control-row">
                <b-col cols="3" class="player-col">
                    <b-aspect aspect="16:9">
                        <video-player v-if="videoOptions.sources" reference="videoPlayer" :options="videoOptions" />
                    </b-aspect>
                </b-col>
                <b-col class="control-col">
                    <b-row class="control-col">
                        <b-col cols="8">
                            <b-row class="stats-row">
                                <b-col class="time-col">
                                    <div class="time-str">
                                        {{ timeStr }}
                                    </div>
                                </b-col>
                                <b-col class="time-col">
                                    <div class="time-str">
                                        {{ timeLeft }}
                                    </div>
                                </b-col>
                                <div class="w-100" />
                                <b-col class="current-clip" align-self="end">
                                    <div class="current-clip-text">
                                        {{ currentClip | filename }}
                                    </div>
                                    <div class="current-clip-progress">
                                        <b-progress :value="progressValue" variant="warning" />
                                    </div>
                                </b-col>
                            </b-row>
                        </b-col>
                        <b-col cols="4" class="control-unit-col">
                            <b-row class="control-unit-row">
                                <b-col>
                                    <div>
                                        <b-button class="control-button control-button-play" variant="primary">
                                            <b-icon-play />
                                        </b-button>
                                    </div>
                                </b-col>
                                <b-col>
                                    <div>
                                        <b-button class="control-button control-button-stop" variant="primary">
                                            <b-icon-stop />
                                        </b-button>
                                    </div>
                                </b-col>
                                <div class="w-100" />
                                <b-col>
                                    <div>
                                        <b-button class="control-button control-button-reload" variant="primary">
                                            <b-icon-arrow-repeat />
                                        </b-button>
                                    </div>
                                </b-col>
                                <b-col>
                                    <div>
                                        <b-button class="control-button control-button-restart" variant="primary">
                                            <b-icon-arrow-clockwise />
                                        </b-button>
                                    </div>
                                </b-col>
                            </b-row>
                        </b-col>
                    </b-row>
                </b-col>
            </b-row>
            <b-row class="date-row">
                <b-col>
                    <b-datepicker v-model="today" size="sm" class="date-div" offset="-35px" />
                </b-col>
            </b-row>
            <splitpanes class="list-row default-theme pane-row">
                <pane min-size="20" size="24">
                    <loading
                        :active.sync="isLoading"
                        :can-cancel="false"
                        :is-full-page="false"
                        background-color="#485159"
                        color="#ff9c36"
                    />

                    <div v-if="folderTree.tree" class="browser-div">
                        <div>
                            <b-breadcrumb>
                                <b-breadcrumb-item
                                    v-for="(crumb, index) in crumbs"
                                    :key="crumb.key"
                                    :active="index === crumbs.length - 1"
                                    @click="getPath(extensions, crumb.path)"
                                >
                                    {{ crumb.text }}
                                </b-breadcrumb-item>
                            </b-breadcrumb>
                        </div>

                        <perfect-scrollbar>
                            <!-- class="browser-list" -->
                            <b-list-group>
                                <b-list-group-item
                                    v-for="folder in folderTree.tree[1]"
                                    :key="folder.key"
                                    class="browser-item"
                                >
                                    <b-link @click="getPath(extensions, `${folderTree.tree[0]}/${folder}`)">
                                        <b-icon-folder-fill class="browser-icons" /> {{ folder }}
                                    </b-link>
                                </b-list-group-item>
                                <b-list-group-item
                                    v-for="file in folderTree.tree[2]"
                                    :key="file.key"
                                    class="browser-item"
                                >
                                    <b-row>
                                        <b-col cols="1" class="browser-icons-col">
                                            <b-icon-film class="browser-icons" />
                                        </b-col>
                                        <b-col class="browser-item-text">
                                            {{ file.file }}
                                        </b-col>
                                        <b-col cols="1" class="browser-play-col">
                                            <b-link @click="showModal(`/${folderTree.tree[0]}/${file.file}`)">
                                                <b-icon-play-fill />
                                            </b-link>
                                        </b-col>
                                        <b-col cols="1" class="browser-dur-col">
                                            <span class="duration">{{ file.duration | toMin }}</span>
                                        </b-col>
                                    </b-row>
                                </b-list-group-item>
                            </b-list-group>
                        </perfect-scrollbar>
                    </div>
                </pane>
                <pane>
                    <div class="playlist-container">
                        <b-list-group>
                            <b-list-group-item>
                                <b-row class="playlist-row">
                                    <b-col cols="1" class="timecode">
                                        Start
                                    </b-col>
                                    <b-col cols="6">
                                        File
                                    </b-col>
                                    <b-col cols="1" class="text-center">
                                        Play
                                    </b-col>
                                    <b-col cols="1" class="timecode">
                                        Duration
                                    </b-col>
                                    <b-col cols="1" class="timecode">
                                        In
                                    </b-col>
                                    <b-col cols="1" class="timecode">
                                        Out
                                    </b-col>
                                    <b-col cols="1" class="text-center">
                                        Delete
                                    </b-col>
                                </b-row>
                            </b-list-group-item>
                        </b-list-group>
                        <perfect-scrollbar>
                            <b-list-group>
                                <b-list-group-item v-for="item in playlist" :key="item.key">
                                    <b-row class="playlist-row" :data-in="item.in" :data-out="item.out">
                                        <b-col cols="1" class="timecode">
                                            {{ item.begin | secondsToTime }}
                                        </b-col>
                                        <b-col cols="6">
                                            {{ item.source | filename }}
                                        </b-col>
                                        <b-col cols="1" class="text-center">
                                            <b-link @click="showModal(item.source)">
                                                <b-icon-play-fill />
                                            </b-link>
                                        </b-col>
                                        <b-col cols="1" text class="timecode">
                                            {{ item.duration | secondsToTime }}
                                        </b-col>
                                        <b-col cols="1" class="timecode">
                                            {{ item.in | secondsToTime }}
                                        </b-col>
                                        <b-col cols="1" class="timecode">
                                            {{ item.out | secondsToTime }}
                                        </b-col>
                                        <b-col cols="1" class="text-center">
                                            <b-icon-x-circle-fill />
                                        </b-col>
                                    </b-row>
                                </b-list-group-item>
                            </b-list-group>
                        </perfect-scrollbar>
                    </div>
                </pane>
            </splitpanes>
        </b-container>
        <b-modal
            id="preview-modal"
            ref="prev-modal"
            size="xl"
            centered
            :title="`Preview: ${previewSource}`"
            hide-footer
        >
            <video-player v-if="previewOptions" reference="previewPlayer" :options="previewOptions" />
        </b-modal>
    </div>
</template>

<script>
import { mapState } from 'vuex'
import Menu from '@/components/Menu.vue'

export default {
    name: 'Control',

    components: {
        Menu
    },

    filters: {
        filename (path) {
            if (path) {
                const pathArr = path.split('/')
                return pathArr[pathArr.length - 1]
            }
        },
        secondsToTime (sec) {
            return new Date(sec * 1000).toISOString().substr(11, 8)
        }
    },

    data () {
        return {
            isLoading: false,
            today: this.$dayjs().format('YYYY-MM-DD'),
            extensions: '',
            videoOptions: {},
            previewOptions: {},
            previewComp: null,
            previewSource: ''
        }
    },

    computed: {
        ...mapState('config', ['configGui', 'configPlayout']),
        ...mapState('media', ['crumbs', 'folderTree']),
        ...mapState('playlist', ['playlist', 'timeStr', 'timeLeft', 'currentClip', 'progressValue'])
    },

    watch: {
        today (date) {
            this.getPlaylist()
        }
    },

    async created () {
        await this.getConfig()

        this.extensions = this.configPlayout.storage.extensions.join(' ')

        await this.getPath(this.extensions, '')

        this.videoOptions = {
            liveui: true,
            controls: true,
            suppressNotSupportedError: true,
            autoplay: false,
            preload: 'auto',
            sources: [
                {
                    type: 'application/x-mpegURL',
                    src: this.configGui.player_url
                }
            ]
        }

        await this.getPlaylist()
    },

    methods: {
        async getConfig () {
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('config/getGuiConfig')
            await this.$store.dispatch('config/getPlayoutConfig')
        },
        async getPath (extensions, path) {
            this.isLoading = true
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('media/getTree', { extensions, path })
            this.isLoading = false
        },
        async getPlaylist () {
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('playlist/getPlaylist', { dayStart: this.configPlayout.playlist.day_start, date: this.today })
        },
        showModal (src) {
            this.previewSource = src.split('/').slice(-1)[0]
            const ext = this.previewSource.split('.').slice(-1)[0]
            this.previewOptions = {
                liveui: false,
                controls: true,
                suppressNotSupportedError: true,
                autoplay: false,
                preload: 'auto',
                sources: [
                    {
                        type: `video/${ext}`,
                        src: encodeURIComponent(src)
                    }
                ]
            }
            this.$root.$emit('bv::show::modal', 'preview-modal')
        }
    }
}
</script>

<style lang="scss">
.control-container {
    width: auto;
    max-width: 100%;
    height: calc(100% - 40px);
}

.control-row {
    min-height: 254px;
}

.player-col {
    max-width: 542px;
    min-width: 380px;
    margin-bottom: 6px;
}

.control-col {
    height: 100%;
    min-height: 254px;
}

.stats-row {
    height: 100%;
    min-width: 470px;
}

.time-col {
    position: relative;
    background: #32383E;
    height: calc(50% - 3px);
    margin: 0 6px 6px 0;
    text-align: center;
    border-radius: 0.25rem;
}

.time-str {
    position: relative;
    top: 50%;
    -webkit-transform: translateY(-50%);
    -ms-transform: translateY(-50%);
    transform: translateY(-50%);
    font-family: 'DigitalNumbers-Regular';
    font-size: 4.5em;
    letter-spacing: -.18em;
    padding-right: 14px;
}

.current-clip {
    background: #32383E;
    height: calc(50% - 3px);
    margin-right: 6px;
    padding: 15px;
    border-radius: 0.25rem;
}

.current-clip-text {
    height: 70%;
}

.current-clip-progress {
    top: 80%;
}

.control-unit-col {
    min-width: 380px;
}

.control-unit-row {
    background: #32383E;
    height: 100%;
    margin-right: 0;
    border-radius: 0.25rem;
    text-align: center;
}

.control-unit-row .col {
    position: relative;
    height: 50%;
    min-height: 90px;
}

.control-unit-row .col div {
    position: relative;
    top: 50%;
    -webkit-transform: translateY(-50%);
    -ms-transform: translateY(-50%);
    transform: translateY(-50%);
    height: 80%;
}

.control-button {
    font-size: 3em;
    line-height: 0;
    width: 80%;
    height: 100%;
}

.control-button-play {
    color: #43c32e;
}
.control-button-stop {
    color: #d01111;
}
.control-button-reload {
    color: #ed7c06;
}
.control-button-restart {
    color: #f6e502;
}

@media (max-width: 1555px) {
    .control-col {
        height: 100%;
        min-height: 294px;
    }
    .time-col {
        height: calc(33.3% - 3px);
        margin: 0 0 6px 0;
    }
    .current-clip {
        height: calc(33.3% - 3px);
        margin-right: 0;
    }
    .control-unit-row {
        margin-right: -15px;
    }
    .control-unit-col {
        flex: 0 0 66.6666666667%;
        max-width: 66.6666666667%;
        margin: 9px 0 0 0;
    }
}

@media (max-width: 849px) {
    .stats-row {
        min-width: 380px;
    }
}

.date-row {
    height: 44px;
    padding-top: 5px;
}

.list-row {
    height: calc(100% - 40px - 254px - 46px);
    min-height: 300px;
}

.pane-row {
    margin: 0;
}

.browser-div {
    width: 100%;
    max-height: 100%;
}

.date-div {
    width: 250px;
    float: right;
}

.playlist-container {
    width: 100%;
    height: 100%;
}

.ps__thumb-x {
    display: none;
}

.splitpanes__pane {
    width: 30%;
}

.splitpanes.default-theme .splitpanes__pane {
    background-color: $dark;
    box-shadow: 0 0 10px rgba(0, 0, 0, .2) inset;
    justify-content: center;
    align-items: center;
    display: flex;
    position: relative;
}

.default-theme.splitpanes--vertical > .splitpanes__splitter, .default-theme .splitpanes--vertical > .splitpanes__splitter {
    border-left: 1px solid $dark;
}

.splitpanes.default-theme .splitpanes__splitter {
    background-color: $dark;
}

.splitpanes.default-theme .splitpanes__splitter::after, .splitpanes.default-theme .splitpanes__splitter::before {
    background-color: rgba(136, 136, 136, 0.38);
}

.timecode {
    min-width: 56px;
}

</style>
