<template>
    <div>
        <b-container class="browser">
            <div v-if="folderTree.tree" class="browser">
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

                <b-row>
                    <b-col class="folder-col">
                        <div class="browser-div">
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
                            </b-list-group>
                        </div>
                    </b-col>
                    <b-col class="files-col">
                        <div class="browser-div">
                            <b-list-group>
                                <b-list-group-item
                                    v-for="file in folderTree.tree[2]"
                                    :key="file.key"
                                    class="browser-item"
                                >
                                    <b-link>
                                        <b-icon-film class="browser-icons" />  {{ file }}
                                    </b-link>
                                </b-list-group-item>
                            </b-list-group>
                        </div>
                    </b-col>
                </b-row>
            </div>
        </b-container>
    </div>
</template>

<script>
import { mapState } from 'vuex'

export default {
    name: 'Media',

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
.browser {
    width: 100%;
    max-width: 100%;
}

.folder-col {
    min-width: 320px;
    max-width: 460px;
}

.folder {
    padding: .3em;
}

.files-col {
    min-width: 320px;
}

.browser-div {
    background: #30363d;
    height: 100%;
    border: 1px solid #000;
    border-radius: 5px;
}

</style>
