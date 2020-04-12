<template>
    <div>
        <b-container class="control-container">
            <b-row>
                <b-col cols="3">
                    <b-aspect class="player-col" aspect="16:9">
                        Player
                    </b-aspect>
                </b-col>
                <b-col cols="9" class="control-col">
                    control
                </b-col>
            </b-row>
            <splitpanes class="default-theme pane-row">
                <pane min-size="20" size="24">
                    <div v-if="folderTree.tree" class="browser-div">
                        <div>
                            <b-breadcrumb>
                                <b-breadcrumb-item
                                    v-for="(crumb, index) in crumbs"
                                    :key="crumb.key"
                                    :active="index === crumbs.length - 1"
                                    @click="getPath(crumb.path)"
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
                                    <b-link @click="getPath(`${folderTree.tree[0]}/${folder}`)">
                                        <b-icon-folder-fill class="browser-icons" /> {{ folder }}
                                    </b-link>
                                </b-list-group-item>
                                <b-list-group-item
                                    v-for="file in folderTree.tree[2]"
                                    :key="file.key"
                                    class="browser-item"
                                >
                                    <b-link>
                                        <b-icon-film class="browser-icons" /> {{ file }}
                                    </b-link>
                                </b-list-group-item>
                            </b-list-group>
                        </perfect-scrollbar>
                    </div>
                </pane>
                <pane>
                    <div class="playlist-container">
                        <b-datepicker v-model="today" class="date-div" />
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
                            <b-list-group v-if="playlist">
                                <b-list-group-item v-for="item in playlist.program" :key="item.key">
                                    <b-row class="playlist-row">
                                        <b-col cols="1">
                                            {{ item.begin | secondsToTime }}
                                        </b-col>
                                        <b-col cols="6">
                                            {{ item.source | filename }}
                                        </b-col>
                                        <b-col cols="1" class="text-center">
                                            <b-icon-play-fill />
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
    </div>
</template>

<script>
import { mapState } from 'vuex'

export default {
    name: 'Control',

    components: {},

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
            today: this.$dayjs().format('YYYY-MM-DD')
        }
    },

    computed: {
        ...mapState('config', ['config']),
        ...mapState('media', ['crumbs', 'folderTree']),
        ...mapState('playlist', ['playlist'])
    },

    watch: {
        today (date) {
            this.getPlaylist()
        }
    },

    async created () {
        await this.getPath('')
        await this.getConfig()
        await this.getPlaylist()
    },

    methods: {
        async getConfig () {
            await this.$store.dispatch('config/getConfig')
        },
        async getPath (path) {
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('media/getTree', path)
        },
        async getPlaylist () {
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('playlist/getPlaylist', { dayStart: this.config.playlist.day_start, date: this.today })
        }
    }
}
</script>

<style lang="scss">
.control-container {
    width: auto;
    max-width: 100%;
    margin: .5em;
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
}

.playlist-container {
    width: 100%;
}

.browser-div .ps, .playlist-container .ps {
    height: 600px;
}

.browser-list {
    max-height: 600px;
    overflow-y: scroll;
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
