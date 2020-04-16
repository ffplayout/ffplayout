<template>
    <div>
        <Menu />
        <b-container class="control-container">
            <b-row>
                <b-col cols="3">
                    <b-aspect class="player-col" aspect="16:9">
                        <video-player v-if="videoOptions.sources" reference="videoPlayer" :options="videoOptions" />
                    </b-aspect>
                </b-col>
                <b-col cols="9" class="control-col">
                    control
                </b-col>
            </b-row>
            <b-row>
                <b-col>
                    <b-datepicker v-model="today" size="sm" class="date-div" offset="-35px" />
                </b-col>
            </b-row>
            <splitpanes class="default-theme pane-row">
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
                                    <b-link>
                                        <b-row>
                                            <b-col cols="1" class="browser-icons-col">
                                                <b-icon-film class="browser-icons" />
                                            </b-col>
                                            <b-col class="browser-item-text">
                                                {{ file.file }}
                                            </b-col>
                                            <b-col cols="1" class="browser-dur-col">
                                                <span class="duration">{{ file.duration | toMin }}</span>
                                            </b-col>
                                        </b-row>
                                    </b-link>
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
                                    <b-col cols="1">
                                        Start
                                    </b-col>
                                    <b-col cols="6">
                                        File
                                    </b-col>
                                    <b-col cols="1" class="text-center">
                                        Play
                                    </b-col>
                                    <b-col cols="1">
                                        Duration
                                    </b-col>
                                    <b-col cols="1">
                                        In
                                    </b-col>
                                    <b-col cols="1">
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
                                    <b-row class="playlist-row">
                                        <b-col cols="1">
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
                                        <b-col cols="1" text>
                                            {{ item.duration | secondsToTime }}
                                        </b-col>
                                        <b-col cols="1">
                                            {{ item.in | secondsToTime }}
                                        </b-col>
                                        <b-col cols="1">
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
            title="Preview"
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
            const pathArr = path.split('/')
            return pathArr[pathArr.length - 1]
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
        ...mapState('playlist', ['playlist'])
    },

    watch: {
        today (date) {
            this.getPlaylist()
        }
    },

    async created () {
        await this.getConfig()
        await this.getPlaylist()

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
            this.previewOptions = {
                liveui: false,
                controls: true,
                suppressNotSupportedError: true,
                autoplay: false,
                preload: 'auto',
                sources: [
                    {
                        type: 'video/mp4',
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
}

.player-col {
    background: black;
    min-width: 300px;
}

.pane-row {
    margin: 8px 0 0 0;
}

.browser-div {
    width: 100%;
}

.date-div {
    width: 250px;
    float: right;
}

.playlist-container {
    width: 100%;
}

.browser-icons-col {
    max-width: 10px;
}

.browser-dur-col {
    min-width: 110px;
}

.browser-div .ps, .playlist-container .ps {
    height: 600px;
}

.browser-list {
    max-height: 600px;
    overflow-y: scroll;
}

.browser-item-text {
    display: inline-block;
    max-width: 95%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
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

</style>
