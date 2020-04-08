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

                        <b-list-group class="browser-list">
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
                    </div>
                </pane>
                <pane>
                    <h2>Playlist</h2>
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

    computed: {
        ...mapState('media', ['crumbs', 'folderTree'])
    },

    created () {
        this.getPath('')
    },

    methods: {
        async getPath (path) {
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('media/getTree', path)
        }
    }
}
</script>

<style lang="scss">
.control-container {
    max-width: 100%;
    margin: .5em;
    padding: 0;
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
