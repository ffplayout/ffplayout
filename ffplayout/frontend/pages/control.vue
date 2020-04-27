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
                                        <b-button
                                            v-b-tooltip.hover
                                            title="Start Playout Service"
                                            class="control-button control-button-play"
                                            :class="isPlaying"
                                            variant="primary"
                                            @click="playoutControl('start')"
                                        >
                                            <b-icon-play />
                                        </b-button>
                                    </div>
                                </b-col>
                                <b-col>
                                    <div>
                                        <b-button
                                            v-b-tooltip.hover
                                            title="Stop Playout Service"
                                            class="control-button control-button-stop"
                                            variant="primary"
                                            @click="playoutControl('stop')"
                                        >
                                            <b-icon-stop />
                                        </b-button>
                                    </div>
                                </b-col>
                                <div class="w-100" />
                                <b-col>
                                    <div>
                                        <b-button
                                            v-b-tooltip.hover
                                            title="Reload Playout Service"
                                            class="control-button control-button-reload"
                                            variant="primary"
                                            @click="playoutControl('reload')"
                                        >
                                            <b-icon-arrow-repeat />
                                        </b-button>
                                    </div>
                                </b-col>
                                <b-col>
                                    <div>
                                        <b-button
                                            v-b-tooltip.hover
                                            title="Restart Playout Service"
                                            class="control-button control-button-restart"
                                            variant="primary"
                                            @click="playoutControl('restart')"
                                        >
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
                                <draggable
                                    :list="folderTree.tree[2]"
                                    :clone="cloneClip"
                                    :group="{ name: 'playlist', pull: 'clone', put: false }"
                                    :sort="false"
                                >
                                    <b-list-group-item
                                        v-for="file in folderTree.tree[2]"
                                        :key="file.key"
                                        class="browser-item"
                                    >
                                        <b-row>
                                            <b-col cols="1" class="browser-icons-col">
                                                <b-icon-film class="browser-icons" />
                                            </b-col>
                                            <b-col class="browser-item-text grabbing">
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
                                </draggable>
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
                                    <b-col>
                                        File
                                    </b-col>
                                    <b-col cols="1" class="text-center playlist-input">
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
                                    <b-col cols="1" class="text-center playlist-input">
                                        Ad
                                    </b-col>
                                    <b-col cols="1" class="text-center playlist-input">
                                        Delete
                                    </b-col>
                                </b-row>
                            </b-list-group-item>
                        </b-list-group>
                        <perfect-scrollbar>
                            <b-list-group>
                                <draggable
                                    v-model="playlist"
                                    group="playlist"
                                    @start="drag=true"
                                    @end="drag=false"
                                >
                                    <b-list-group-item v-for="(item, index) in playlist" :key="item.key" class="playlist-item">
                                        <b-row class="playlist-row">
                                            <b-col cols="1" class="timecode">
                                                {{ item.begin | secondsToTime }}
                                            </b-col>
                                            <b-col class="grabbing">
                                                {{ item.source | filename }}
                                            </b-col>
                                            <b-col cols="1" class="text-center playlist-input">
                                                <b-link @click="showModal(item.source)">
                                                    <b-icon-play-fill />
                                                </b-link>
                                            </b-col>
                                            <b-col cols="1" text class="timecode">
                                                {{ item.duration | secondsToTime }}
                                            </b-col>
                                            <b-col cols="1" class="timecode">
                                                <b-form-input :value="item.in | secondsToTime" size="sm" @input="changeTime('in', index, $event)" />
                                            </b-col>
                                            <b-col cols="1" class="timecode">
                                                <b-form-input :value="item.out | secondsToTime" size="sm" @input="changeTime('out', index, $event)" />
                                            </b-col>
                                            <b-col cols="1" class="text-center playlist-input">
                                                <b-form-checkbox
                                                    v-model="item.category"
                                                    value="advertisement"
                                                    :unchecked-value="item.category"
                                                />
                                            </b-col>
                                            <b-col cols="1" class="text-center playlist-input">
                                                <b-link @click="removeItemFromPlaylist(index)">
                                                    <b-icon-x-circle-fill />
                                                </b-link>
                                            </b-col>
                                        </b-row>
                                    </b-list-group-item>
                                </draggable>
                            </b-list-group>
                        </perfect-scrollbar>
                    </div>
                </pane>
            </splitpanes>
            <b-button-group class="media-button">
                <b-button v-b-tooltip.hover title="Reset Playlist" variant="primary" @click="resetPlaylist()">
                    <b-icon-arrow-counterclockwise />
                </b-button>
                <b-button v-b-tooltip.hover title="Save Playlist" variant="primary" @click="savePlaylist()">
                    <b-icon-download />
                </b-button>
            </b-button-group>
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
            let hours = Math.floor(sec / 3600)
            sec %= 3600
            let minutes = Math.floor(sec / 60)
            let seconds = sec % 60

            minutes = String(minutes).padStart(2, '0')
            hours = String(hours).padStart(2, '0')
            seconds = String(parseInt(seconds)).padStart(2, '0')
            return hours + ':' + minutes + ':' + seconds
        }
    },

    data () {
        return {
            isLoading: false,
            isPlaying: '',
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
        ...mapState('playlist', ['timeStr', 'timeLeft', 'currentClip', 'progressValue']),
        playlist: {
            get () {
                return this.$store.state.playlist.playlist
            },
            set (list) {
                this.$store.commit('playlist/UPDATE_PLAYLIST', this.$processPlaylist(
                    this.configPlayout.playlist.day_start, list))
            }
        }
    },

    watch: {
        today (date) {
            this.getPlaylist()
        }
    },

    async created () {
        await this.getConfig()

        await this.getStatus()

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
        async getStatus () {
            await this.$store.dispatch('auth/inspectToken')

            const status = await this.$axios.post(
                'api/system/',
                { run: 'status' },
                { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken } }
            )

            if (status.data.data && status.data.data === 'active') {
                this.isPlaying = 'is-playing'
            } else {
                this.isPlaying = ''
            }
        },
        async playoutControl (state) {
            await this.$store.dispatch('auth/inspectToken')
            await this.$axios.post(
                'api/system/',
                { run: state },
                { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken } }
            )

            await this.getStatus()
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
        },
        cloneClip ({ file, duration }) {
            return {
                source: `/${this.folderTree.tree[0]}/${file}`,
                in: 0,
                out: duration,
                duration
            }
        },
        changeTime (pos, index, input) {
            if (input.match(/(?:[01]\d|2[0123]):(?:[012345]\d):(?:[012345]\d)/gm)) {
                const sec = this.$timeToSeconds(input)

                if (pos === 'in') {
                    this.playlist[index].in = sec
                } else if (pos === 'out') {
                    this.playlist[index].out = sec
                }

                this.$store.commit('playlist/UPDATE_PLAYLIST', this.$processPlaylist(
                    this.configPlayout.playlist.day_start, this.playlist))
            }
        },
        removeItemFromPlaylist (index) {
            this.playlist.splice(index, 1)
        },
        async resetPlaylist () {
            await this.$store.dispatch('playlist/getPlaylist', { dayStart: this.configPlayout.playlist.day_start, date: this.today })
        },
        async savePlaylist () {
            await this.$store.dispatch('auth/inspectToken')
            this.$store.commit('playlist/UPDATE_PLAYLIST', this.$processPlaylist(
                this.configPlayout.playlist.day_start, this.playlist))

            await this.$axios.post(
                'api/playlist/',
                { data: { channel: this.$store.state.playlist.playlistChannel, date: this.today, program: this.playlist } },
                { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken } }
            )
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
    padding-top: 1em;
    font-weight: bold;
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

.control-button:hover {
    background-image: linear-gradient(#3b4046, #2c3034 60%, #24272a) !important;
}

.control-button-play {
    color: #43c32e;
}

.is-playing {
    box-shadow: 0 0 15px  #43c32e;
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
    height: calc(100% - 40px - 254px - 46px - 70px);
    min-height: 300px;
}

.pane-row {
    margin: 0;
}

.browser-div {
    width: 100%;
    max-height: 100%;
}

.browser-div .ps {
    padding-left: .4em;
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
    max-width: 90px;
}

.playlist-input {
    min-width: 35px;
    max-width: 60px;
}

.timecode input {
    border-color: #515763;
}

.playlist-item:nth-of-type(even), .playlist-item:nth-of-type(even) div .timecode input {
    background-color: #3b424a;
}

.playlist-item:nth-of-type(even):hover {
    background-color: #1C1E22;
}

</style>
