<template>
    <div>
        <b-container class="control-container">
            <b-row>
                <b-col cols="3" class="player-col">
                    <b-aspect aspect="16:9">
                        Player
                    </b-aspect>
                </b-col>
                <b-col cols="9" class="control-col">
                    control
                </b-col>
            </b-row>
            <b-row>
                <b-col cols="3" class="browser-col">
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
                                    {{ folder }}
                                </b-link>
                            </b-list-group-item>
                        </b-list-group>
                    </div>
                </b-col>
                <b-col cols="9" class="playlist-col">
                    playlist
                </b-col>
            </b-row>
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

<style>
.control-container {
    max-width: 100%;
    margin: .5em;
}

.player-col {
    background: black;
    min-width: 300px;
}

.browser-list {
    max-height: 600px;
    overflow-y: scroll;
}

</style>
