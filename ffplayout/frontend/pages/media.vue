<template>
    <div>
        <Menu />
        <b-container class="browser">
            <div v-if="folderTree.tree" class="browser">
                <div class="bread-div">
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

                <splitpanes class="browser-row default-theme pane-row">
                    <pane min-size="20" size="24">
                        <div class="browser-div">
                            <perfect-scrollbar>
                                <b-list-group class="folder-list">
                                    <b-list-group-item
                                        v-for="folder in folderTree.tree[1]"
                                        :key="folder.key"
                                        class="browser-item folder"
                                    >
                                        <b-row>
                                            <b-col cols="1" class="browser-icons-col">
                                                <b-icon-folder-fill class="browser-icons" />
                                            </b-col>
                                            <b-col class="browser-item-text">
                                                <b-link @click="getPath(extensions, `${folderTree.tree[0]}/${folder}`)">
                                                    {{ folder }}
                                                </b-link>
                                            </b-col>
                                            <b-col v-if="folder !== '..'" cols="1" class="folder-delete">
                                                <b-link @click="showDeleteModal('Folder', `/${folderTree.tree[0]}/${folder}`)">
                                                    <b-icon-x-circle-fill />
                                                </b-link>
                                            </b-col>
                                        </b-row>
                                    </b-list-group-item>
                                </b-list-group>
                            </perfect-scrollbar>
                        </div>
                    </pane>
                    <pane class="files-col">
                        <loading
                            :active.sync="isLoading"
                            :can-cancel="false"
                            :is-full-page="false"
                            background-color="#485159"
                            color="#ff9c36"
                        />
                        <div class="browser-div">
                            <perfect-scrollbar>
                                <b-list-group class="files-list">
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
                                                <b-link @click="showPreviewModal(`/${folderTree.tree[0]}/${file.file}`)">
                                                    <b-icon-play-fill />
                                                </b-link>
                                            </b-col>
                                            <b-col cols="1" class="browser-dur-col">
                                                <span class="duration">{{ file.duration | toMin }}</span>
                                            </b-col>
                                            <b-col cols="1" class="text-center">
                                                <b-link @click="showDeleteModal('File', `/${folderTree.tree[0]}/${file.file}`)">
                                                    <b-icon-x-circle-fill />
                                                </b-link>
                                            </b-col>
                                        </b-row>
                                    </b-list-group-item>
                                </b-list-group>
                            </perfect-scrollbar>
                        </div>
                    </pane>
                </splitpanes>
                <b-button-group class="media-button">
                    <b-button variant="primary" @click="showCreateFolderModal()">
                        <b-icon-folder-plus />
                    </b-button>
                    <b-button variant="primary" @click="showUploadModal()">
                        <b-icon-upload />
                    </b-button>
                </b-button-group>
            </div>
        </b-container>
        <b-modal
            id="preview-modal"
            ref="prev-modal"
            size="xl"
            centered
            :title="`Preview: ${previewName}`"
            hide-footer
        >
            <b-img v-if="isImage" :src="previewSource" fluid :alt="previewName" />
            <video-player v-else-if="!isImage && previewOptions" reference="previewPlayer" :options="previewOptions" />
        </b-modal>

        <b-modal
            id="folder-modal"
            ref="folder-modal"
            size="xl"
            centered
            title="Create Folder"
            hide-footer
        >
            <b-form @submit="onSubmitCreateFolder" @reset="onCancelCreateFolder">
                <b-form-input
                    id="folder-name"
                    v-model="folderName"
                    type="text"
                    required
                    placeholder="Enter a unique folder name"
                />
                <div class="media-button">
                    <b-button type="submit" variant="primary">
                        Create
                    </b-button>
                    <b-button type="reset" variant="primary">
                        Cancel
                    </b-button>
                </div>
            </b-form>
        </b-modal>

        <b-modal
            id="upload-modal"
            ref="up-modal"
            size="xl"
            centered
            title="File Upload"
            hide-footer
        >
            <b-form @submit="onSubmitUpload" @reset="onResetUpload">
                <b-form-file
                    v-model="inputFiles"
                    :state="Boolean(inputFiles)"
                    placeholder="Choose files or drop them here..."
                    drop-placeholder="Drop files here..."
                    multiple
                    :accept="extensions.replace(/ /g, ', ')"
                    :file-name-formatter="formatNames"
                />
                <b-row>
                    <b-col cols="10">
                        <b-row class="progress-row">
                            <b-col cols="1">
                                Overall:
                            </b-col>
                            <b-col cols="10">
                                <b-progress :value="overallProgress" />
                            </b-col>
                            <div class="w-100" />
                            <b-col cols="1">
                                Current:
                            </b-col>
                            <b-col cols="10">
                                <b-progress :value="currentProgress" />
                            </b-col>
                            <div class="w-100" />
                            <b-col cols="1">
                                Uploading:
                            </b-col>
                            <b-col cols="10">
                                <strong>{{ uploadTask }}</strong>
                            </b-col>
                        </b-row>
                    </b-col>
                    <b-col cols="2">
                        <div class="media-button">
                            <b-button type="submit" variant="primary">
                                Upload
                            </b-button>
                            <b-button type="reset" variant="primary">
                                Cancel
                            </b-button>
                        </div>
                    </b-col>
                </b-row>
            </b-form>
        </b-modal>
        <b-modal id="delete-modal" :title="`Delete ${deleteType}`" centered hide-footer>
            <p>
                Are you sure that you want to delete:<br>
                <strong>{{ previewName }}</strong>
            </p>
            <div class="media-button">
                <b-button variant="primary" @click="deleteFileOrFolder()">
                    Ok
                </b-button>
                <b-button variant="primary" @click="cancelDelete()">
                    Cancel
                </b-button>
            </div>
        </b-modal>
    </div>
</template>

<script>
import { mapState } from 'vuex'
import Menu from '@/components/Menu.vue'

export default {
    name: 'Media',

    components: {
        Menu
    },

    data () {
        return {
            isLoading: false,
            extensions: '',
            folderName: '',
            inputFiles: null,
            previewOptions: {},
            previewComp: null,
            previewName: '',
            previewSource: '',
            deleteType: 'File',
            deleteSource: '',
            isImage: false,
            uploadTask: '',
            overallProgress: 0,
            currentProgress: 0,
            cancelTokenSource: this.$axios.CancelToken.source(),
            lastPath: ''
        }
    },

    computed: {
        ...mapState('config', ['configGui', 'configPlayout']),
        ...mapState('media', ['crumbs', 'folderTree'])
    },

    async created () {
        await this.$store.dispatch('auth/inspectToken')
        await this.$store.dispatch('config/getGuiConfig')
        await this.$store.dispatch('config/getPlayoutConfig')

        this.extensions = [...this.configPlayout.storage.extensions, ...this.configGui.extra_extensions].join(' ')
        this.getPath(this.extensions, '')
    },

    methods: {
        async getPath (extensions, path) {
            this.lastPath = path
            this.isLoading = true
            await this.$store.dispatch('auth/inspectToken')
            await this.$store.dispatch('media/getTree', { extensions, path })
            this.isLoading = false
        },

        showCreateFolderModal () {
            this.$root.$emit('bv::show::modal', 'folder-modal')
        },

        async onSubmitCreateFolder (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')

            await this.$axios.post(
                'api/media/op/',
                { folder: this.folderName, path: this.crumbs.map(e => e.text).join('/') },
                { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken } }
            )

            this.$root.$emit('bv::hide::modal', 'folder-modal')
            this.getPath(this.extensions, this.lastPath)
        },

        onCancelCreateFolder (evt) {
            evt.preventDefault()
            this.$root.$emit('bv::hide::modal', 'folder-modal')
        },

        showUploadModal () {
            this.uploadTask = ''
            this.currentProgress = 0
            this.overallProgress = 0
            this.$root.$emit('bv::show::modal', 'upload-modal')
        },

        formatNames (files) {
            if (files.length === 1) {
                return files[0].name
            } else {
                return `${files.length} files selected`
            }
        },

        async onSubmitUpload (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')

            const uploadProgress = fileName => (progressEvent) => {
                const progress = Math.round((progressEvent.loaded * 100) / progressEvent.total)
                this.currentProgress = progress
            }

            for (const [i, file] of this.inputFiles.entries()) {
                this.uploadTask = file.name

                const config = {
                    onUploadProgress: uploadProgress(file.name),
                    cancelToken: this.cancelTokenSource.token,
                    headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken }
                }

                await this.$axios.put(
                    `api/media/upload/${encodeURIComponent(file.name)}?path=${encodeURIComponent(this.crumbs.map(e => e.text).join('/'))}`,
                    file,
                    config
                )
                    .then(
                        this.overallProgress = (i + 1) * 100 / this.inputFiles.length,
                        this.currentProgress = 0,
                        await this.$store.dispatch('auth/inspectToken')
                    )
                    .catch(err => console.log(err))
            }

            this.uploadTask = 'Done...'
            this.getPath(this.extensions, this.lastPath)
            this.$root.$emit('bv::hide::modal', 'upload-modal')
        },

        onResetUpload (evt) {
            evt.preventDefault()
            this.inputFiles = null
            this.overallProgress = 0
            this.currentProgress = 0
            this.uploadTask = ''

            this.cancelTokenSource.cancel('Upload cancelled')
            this.getPath(this.extensions, this.lastPath)

            this.$root.$emit('bv::hide::modal', 'upload-modal')
        },

        showPreviewModal (src) {
            this.previewSource = src
            this.previewName = src.split('/').slice(-1)[0]
            const ext = this.previewName.split('.').slice(-1)[0]

            if (this.configPlayout.storage.extensions.includes(`.${ext}`)) {
                this.isImage = false
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
            } else {
                this.isImage = true
            }
            this.$root.$emit('bv::show::modal', 'preview-modal')
        },

        showDeleteModal (type, src) {
            this.deleteSource = src

            if (type === 'File') {
                this.previewName = src.split('/').slice(-1)[0]
            } else {
                this.previewName = src
            }

            this.deleteType = type
            this.$root.$emit('bv::show::modal', 'delete-modal')
        },

        async deleteFileOrFolder () {
            await this.$store.dispatch('auth/inspectToken')
            let file
            let pathName

            if (this.deleteType === 'File') {
                file = this.deleteSource.split('/').slice(-1)[0]
                pathName = this.deleteSource.substring(0, this.deleteSource.lastIndexOf('/') + 1)
            } else {
                file = null
                pathName = this.deleteSource
            }

            await this.$axios.delete(
                `api/media/op/?file=${encodeURIComponent(file)}&path=${encodeURIComponent(pathName)}`,
                { headers: { Authorization: 'Bearer ' + this.$store.state.auth.jwtToken } }
            )
                .catch(err => console.log(err))

            this.$root.$emit('bv::hide::modal', 'delete-modal')

            this.getPath(this.extensions, this.lastPath)
        },

        cancelDelete () {
            this.deleteSource = ''
            this.$root.$emit('bv::hide::modal', 'delete-modal')
        }
    }
}
</script>

<style>
.browser {
    width: 100%;
    max-width: 100%;
    height: calc(100% - 40px);
}

.bread-div {
    height: 50px;
}

.browser-div {
    background: #30363d;
    height: 100%;
    border: 1px solid #000;
    border-radius: 5px;
}

.browser-row {
    height: calc(100% - 90px);
    min-height: 50px;
}

.folder-col {
    min-width: 320px;
    max-width: 460px;
    height: 100%;
}

.folder:hover > div > .folder-delete {
    display: inline;
}

.folder-list {
    height: 100%;
    padding: .5em;
}

.folder-delete {
    margin-right: .5em;
    display: none;
}

.files-col {
    min-width: 320px;
    height: 100%;
}

.files-list {
    width: 99.5%;
    height: 100%;
    padding: .5em;
}

.media-button {
    float: right;
    margin-top: 1em;
}

.progress-row {
    margin-top: 1em;
}

.progress-row .col-1 {
    min-width: 60px
}

.progress-row .col-10 {
    margin: auto 0 auto 0
}
</style>
