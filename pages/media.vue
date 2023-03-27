<template>
    <div>
        <Menu />
        <div class="container-fluid browser-container">
            <div class="h-100">
                <div>
                    <nav aria-label="breadcrumb">
                        <ol class="breadcrumb">
                            <li
                                class="breadcrumb-item"
                                v-for="(crumb, index) in mediaStore.crumbs"
                                :key="index"
                                :active="index === mediaStore.crumbs.length - 1"
                                @click="getPath(crumb.path)"
                            >
                                <a
                                    v-if="mediaStore.crumbs.length > 1 && mediaStore.crumbs.length - 1 > index"
                                    class="link-secondary"
                                    href="#"
                                >
                                    {{ crumb.text }}
                                </a>
                                <span v-else>{{ crumb.text }}</span>
                            </li>
                        </ol>
                    </nav>
                </div>

                <div class="browser-div">
                    <div v-if="browserIsLoading" class="d-flex justify-content-center loading-overlay">
                        <div class="spinner-border" role="status" />
                    </div>
                    <splitpanes
                        class="pane-row"
                        :class="$route.path === '/player' ? 'browser-splitter' : ''"
                        :horizontal="$route.path === '/player'"
                    >
                        <pane
                            min-size="14"
                            max-size="80"
                            size="24"
                            :style="
                                $route.path === '/player'
                                    ? `height: ${mediaStore.folderTree.folders.length * 47 + 2}px`
                                    : ''
                            "
                        >
                            <ul v-if="mediaStore.folderTree.parent" class="list-group media-browser-scroll m-1">
                                <li
                                    class="list-group-item browser-item"
                                    v-for="folder in mediaStore.folderTree.folders"
                                    :key="folder"
                                >
                                    <div class="row">
                                        <div class="col-1 browser-icons-col">
                                            <i class="bi-folder-fill browser-icons" />
                                        </div>
                                        <div class="col browser-item-text">
                                            <a
                                                class="link-light"
                                                href="#"
                                                @click="getPath(`/${mediaStore.folderTree.source}/${folder}`)"
                                            >
                                                {{ folder }}
                                            </a>
                                        </div>
                                        <div class="col-1 folder-delete">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#deleteModal"
                                                @click="
                                                    deleteName = `/${mediaStore.folderTree.source}/${folder}`.replace(
                                                        /\/[/]+/g,
                                                        '/'
                                                    )
                                                "
                                            >
                                                <i class="bi-x-circle-fill" />
                                            </a>
                                        </div>
                                    </div>
                                </li>
                            </ul>
                        </pane>
                        <pane
                            :style="
                                $route.path === '/player'
                                    ? `height: ${mediaStore.folderTree.files.length * 26 + 2}px`
                                    : ''
                            "
                        >
                            <ul v-if="mediaStore.folderTree.parent" class="list-group media-browser-scroll m-1">
                                <li
                                    v-for="(element, index) in mediaStore.folderTree.files"
                                    :id="`file_${index}`"
                                    class="draggable list-group-item browser-item"
                                    :key="element.name"
                                >
                                    <div class="row">
                                        <div class="col-1 browser-icons-col">
                                            <i
                                                v-if="mediaType(element.name) === 'audio'"
                                                class="bi-music-note-beamed browser-icons"
                                            />
                                            <i
                                                v-else-if="mediaType(element.name) === 'video'"
                                                class="bi-film browser-icons"
                                            />
                                            <i
                                                v-else-if="mediaType(element.name) === 'image'"
                                                class="bi-file-earmark-image browser-icons"
                                            />
                                            <i v-else class="bi-file-binary browser-icons" />
                                        </div>
                                        <div class="col browser-item-text grabbing">
                                            {{ element.name }}
                                        </div>
                                        <div class="col-1 browser-play-col">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#previewModal"
                                                @click="setPreviewData(element.name)"
                                            >
                                                <i class="bi-play-fill" />
                                            </a>
                                        </div>
                                        <div class="col-1 browser-dur-col">
                                            <span class="duration">{{ toMin(element.duration) }}</span>
                                        </div>
                                        <div class="col-1 file-rename">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#renameModal"
                                                @click="
                                                    setRenameValues(
                                                        `/${mediaStore.folderTree.source}/${element.name}`.replace(
                                                            /\/[/]+/g,
                                                            '/'
                                                        )
                                                    )
                                                "
                                            >
                                                <i class="bi-pencil-square" />
                                            </a>
                                        </div>
                                        <div class="col-1 file-delete">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#deleteModal"
                                                @click="
                                                    deleteName =
                                                        `/${mediaStore.folderTree.source}/${element.name}`.replace(
                                                            /\/[/]+/g,
                                                            '/'
                                                        )
                                                "
                                            >
                                                <i class="bi-x-circle-fill" />
                                            </a>
                                        </div>
                                    </div>
                                </li>
                            </ul>
                        </pane>
                    </splitpanes>
                </div>
            </div>

            <div id="previewModal" class="modal" tabindex="-1" aria-labelledby="previewModalLabel" aria-hidden="true">
                <div class="modal-dialog modal-dialog-centered modal-xl">
                    <div class="modal-content">
                        <div class="modal-header">
                            <h1 class="modal-title fs-5" id="previewModalLabel">Preview: {{ previewName }}</h1>
                            <button
                                type="button"
                                class="btn-close"
                                data-bs-dismiss="modal"
                                aria-label="Cancel"
                                @click="closePlayer()"
                            ></button>
                        </div>
                        <div class="modal-body">
                            <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
                            <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
                        </div>
                    </div>
                </div>
            </div>

            <div id="deleteModal" class="modal" tabindex="-1" aria-labelledby="deleteModalLabel" aria-hidden="true">
                <div class="modal-dialog modal-dialog-centered">
                    <div class="modal-content">
                        <div class="modal-header">
                            <h1 class="modal-title fs-5" id="deleteModalLabel">Delete File/Folder</h1>
                            <button
                                type="button"
                                class="btn-close"
                                data-bs-dismiss="modal"
                                aria-label="Cancel"
                            ></button>
                        </div>
                        <div class="modal-body">
                            <p>
                                Are you sure that you want to delete:<br />
                                <strong>{{ deleteName }}</strong>
                            </p>
                        </div>
                        <div class="modal-footer">
                            <button
                                type="reset"
                                class="btn btn-primary"
                                data-bs-dismiss="modal"
                                aria-label="Cancel"
                                @click="deleteName = ''"
                            >
                                Cancel
                            </button>
                            <button
                                type="submit"
                                class="btn btn-primary"
                                data-bs-dismiss="modal"
                                @click="deleteFileOrFolder"
                            >
                                Ok
                            </button>
                        </div>
                    </div>
                </div>
            </div>

            <div id="renameModal" class="modal" tabindex="-1" aria-labelledby="renameModalLabel" aria-hidden="true">
                <div class="modal-dialog modal-dialog-centered">
                    <div class="modal-content">
                        <div class="modal-header">
                            <h1 class="modal-title fs-5" id="renameModalLabel">Rename File</h1>
                            <button
                                type="button"
                                class="btn-close"
                                data-bs-dismiss="modal"
                                aria-label="Cancel"
                            ></button>
                        </div>
                        <form @submit.prevent="onSubmitRenameFile" @reset="onCancelRenameFile">
                            <div class="modal-body">
                                <input type="text" class="form-control" v-model="renameNewName" />
                            </div>
                            <div class="modal-footer">
                                <button
                                    type="reset"
                                    class="btn btn-primary"
                                    data-bs-dismiss="modal"
                                    aria-label="Cancel"
                                >
                                    Cancel
                                </button>
                                <button type="submit" class="btn btn-primary" data-bs-dismiss="modal">Ok</button>
                            </div>
                        </form>
                    </div>
                </div>
            </div>
        </div>
        <div class="btn-group media-button">
            <button
                type="button"
                class="btn btn-primary"
                title="Create Folder"
                data-bs-toggle="modal"
                data-bs-target="#folderModal"
            >
                <i class="bi-folder-plus" />
            </button>
            <button
                type="button"
                class="btn btn-primary"
                title="Upload File"
                data-bs-toggle="modal"
                data-bs-target="#uploadModal"
            >
                <i class="bi-upload" />
            </button>
        </div>

        <div id="folderModal" class="modal" tabindex="-1" aria-labelledby="folderModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="folderModalLabel">Create Folder</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <form @submit.prevent="onSubmitCreateFolder" @reset="onCancelCreateFolder">
                        <div class="modal-body">
                            <input type="text" class="form-control" v-model="folderName" />
                        </div>
                        <div class="modal-footer">
                            <button type="reset" class="btn btn-primary" data-bs-dismiss="modal" aria-label="Cancel">
                                Cancel
                            </button>
                            <button type="submit" class="btn btn-primary" data-bs-dismiss="modal">Ok</button>
                        </div>
                    </form>
                </div>
            </div>
        </div>

        <div
            id="uploadModal"
            ref="uploadModal"
            class="modal"
            tabindex="-1"
            aria-labelledby="uploadModalLabel"
            data-bs-backdrop="static"
        >
            <div class="modal-dialog modal-dialog-centered modal-xl">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="uploadModalLabel">Upload Files</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <form @submit.prevent="onSubmitUpload" @reset="onResetUpload">
                        <div class="modal-body">
                            <input
                                class="form-control"
                                type="file"
                                :accept="extensions"
                                v-on:change="onFileChange"
                                multiple
                            />

                            <div class="row">
                                <div class="col-10">
                                    <div class="row progress-row">
                                        <div class="col-1" style="min-width: 125px">
                                            Overall ({{ currentNumber }}/{{ inputFiles.length }}):
                                        </div>
                                        <div class="col-10">
                                            <div class="progress">
                                                <div
                                                    class="progress-bar bg-warning"
                                                    role="progressbar"
                                                    :aria-valuenow="overallProgress"
                                                    :style="`width: ${overallProgress}%`"
                                                />
                                            </div>
                                        </div>
                                        <div class="w-100" />
                                        <div class="col-1" style="min-width: 125px">Current:</div>
                                        <div class="col-10">
                                            <div class="progress">
                                                <div
                                                    class="progress-bar bg-warning"
                                                    role="progressbar"
                                                    :aria-valuenow="currentProgress"
                                                    :style="`width: ${currentProgress}%`"
                                                />
                                            </div>
                                        </div>
                                        <div class="w-100" />
                                        <div class="col-1" style="min-width: 125px">Uploading:</div>
                                        <div class="col-10">
                                            <strong>{{ uploadTask }}</strong>
                                        </div>
                                    </div>
                                </div>
                                <div class="col-2">
                                    <div class="media-button">
                                        <button type="reset" class="btn btn-primary me-2" data-bs-dismiss="modal">
                                            Cancel
                                        </button>
                                        <button type="submit" class="btn btn-primary">Upload</button>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </form>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'

import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'
import { useMedia } from '~/stores/media'

const { $bootstrap } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const { toMin, mediaType } = stringFormatter()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const { configID } = storeToRefs(useConfig())

definePageMeta({
    middleware: ['auth'],
})

useHead({
    title: 'Media | ffplayout',
})

const browserIsLoading = ref(false)
const deleteName = ref('')
const renameOldName = ref('')
const renameNewName = ref('')
const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)
const uploadModal = ref()
const extensions = ref('')
const folderName = ref('')
const inputFiles = ref([] as File[])
const currentNumber = ref(0)
const uploadTask = ref('')
const overallProgress = ref(0)
const currentProgress = ref(0)
const lastPath = ref('')
const thisUploadModal = ref()
const xhr = ref(new XMLHttpRequest())

onMounted(async () => {
    const exts = [
        ...configStore.configPlayout.storage.extensions,
        ...configStore.configGui[configStore.configID].extra_extensions.split(','),
    ].map((ext) => {
        return `.${ext}`
    })

    extensions.value = exts.join(', ')
    thisUploadModal.value = $bootstrap.Modal.getOrCreateInstance(uploadModal.value)

    if (!mediaStore.folderTree.parent) {
        getPath('')
    }
})

watch([configID], () => {
    getPath('')
})

async function getPath(path: string) {
    browserIsLoading.value = true
    await mediaStore.getTree(path)
    browserIsLoading.value = false
}


function setPreviewData(path: string) {
    /*
        Set path and player options for video preview.
    */
    let fullPath = path
    if (!path.includes('/')) {
        fullPath = `/${mediaStore.folderTree.parent}/${mediaStore.folderTree.source}/${path}`.replace(/\/[/]+/g, '/')
    }

    previewName.value = fullPath.split('/').slice(-1)[0]
    previewUrl.value = encodeURIComponent(`${fullPath}`).replace(/%2F/g, '/')

    const ext = previewName.value.split('.').slice(-1)[0].toLowerCase()
    const fileType = (mediaType(previewName.value) === 'audio') ? `audio/${ext}` : `video/${ext}`

    if (configStore.configPlayout.storage.extensions.includes(`${ext}`)) {
        isVideo.value = true
        previewOpt.value = {
            liveui: false,
            controls: true,
            suppressNotSupportedError: true,
            autoplay: false,
            preload: 'auto',
            sources: [
                {
                    type: fileType,
                    src: previewUrl.value,
                },
            ],
        }
    } else {
        isVideo.value = false
    }
}

async function deleteFileOrFolder() {
    /*
        Delete function, works for files and folders.
    */
    await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/remove/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ source: deleteName.value }),
    })
        .then(async (response) => {
            if (response.status !== 200) {
                indexStore.alertVariant = 'alert-danger'
                indexStore.alertMsg = `${await response.text()}`
                indexStore.showAlert = true
            }
            getPath(mediaStore.folderTree.source)
        })
        .catch((e) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Delete error: ${e}`
            indexStore.showAlert = true
        })

    setTimeout(() => {
        indexStore.alertMsg = ''
        indexStore.showAlert = false
    }, 5000)
}

function setRenameValues(path: string) {
    renameNewName.value = path
    renameOldName.value = path
}

async function onSubmitRenameFile(evt: any) {
    /*
        Form submit for file rename request.
    */
    evt.preventDefault()

    await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/rename/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ source: renameOldName.value, target: renameNewName.value }),
    })
        .then(() => {
            getPath(mediaStore.folderTree.source)
        })
        .catch((e) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Delete error: ${e}`
            indexStore.showAlert = true
        })

    renameOldName.value = ''
    renameNewName.value = ''
}

function onCancelRenameFile(evt: any) {
    evt.preventDefault()

    renameOldName.value = ''
    renameNewName.value = ''
}

function closePlayer() {
    isVideo.value = false
}

async function onSubmitCreateFolder(evt: any) {
    evt.preventDefault()
    const path = `${mediaStore.folderTree.source}/${folderName.value}`.replace(/\/[/]+/g, '/')
    lastPath.value = mediaStore.folderTree.source

    if (mediaStore.folderTree.folders.includes(folderName.value)) {
        indexStore.alertVariant = 'alert-warning'
        indexStore.alertMsg = `Folder "${folderName.value}" exists already!`
        indexStore.showAlert = true

        return
    }

    await $fetch(`/api/file/${configStore.configGui[configStore.configID].id}/create-folder/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ source: path }),
    })
        .then(() => {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Folder create done...'
        })
        .catch((e: string) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Folder create error: ${e}`
        })

    indexStore.showAlert = true
    folderName.value = ''

    setTimeout(() => {
        indexStore.alertMsg = ''
        indexStore.showAlert = false
    }, 2000)

    getPath(lastPath.value)
}

function onCancelCreateFolder(evt: any) {
    evt.preventDefault()
    folderName.value = ''
}

function onFileChange(evt: any) {
    const files = evt.target.files || evt.dataTransfer.files

    if (!files.length) {
        return
    }

    inputFiles.value = files
}

async function onSubmitUpload(evt: any) {
    evt.preventDefault()

    lastPath.value = mediaStore.folderTree.source

    for (let i = 0; i < inputFiles.value.length; i++) {
        const file = inputFiles.value[i]
        uploadTask.value = file.name
        currentNumber.value = i + 1

        const formData = new FormData()
        formData.append(file.name, file)

        xhr.value = new XMLHttpRequest()

        xhr.value.open(
            'PUT',
            `/api/file/${configStore.configGui[configStore.configID].id}/upload/?path=${encodeURIComponent(
                mediaStore.crumbs[mediaStore.crumbs.length - 1].path
            )}`
        )

        xhr.value.setRequestHeader('Authorization', `Bearer ${authStore.jwtToken}`)

        xhr.value.upload.onprogress = function (event) {
            currentProgress.value = Math.round((100 * event.loaded) / event.total)
        }

        xhr.value.upload.onerror = function () {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Upload error: ${xhr.value.status}`
            indexStore.showAlert = true
        }

        // upload completed successfully
        xhr.value.onload = function () {
            overallProgress.value = (currentNumber.value * 100) / inputFiles.value.length
            currentProgress.value = 100
        }

        xhr.value.send(formData)
    }

    uploadTask.value = 'Done...'
    getPath(lastPath.value)

    setTimeout(() => {
        thisUploadModal.value.hide()
        currentNumber.value = 0
        currentProgress.value = 0
        overallProgress.value = 0
        inputFiles.value = []
        indexStore.showAlert = false
    }, 1000)
}

function onResetUpload(evt: any) {
    evt.preventDefault()
    inputFiles.value = []
    overallProgress.value = 0
    currentProgress.value = 0
    uploadTask.value = ''

    xhr.value.abort()
}
</script>

<style lang="scss">

.browser-container .browser-item:hover {
    background-color: $item-hover;

    div > .folder-delete {
        display: inline;
    }
}

.browser-div {
    height: calc(100% - 34px);
}

.folder-delete {
    margin-right: 0.8em;
    display: none;
    min-width: 30px;
}

.file-delete,
.file-rename {
    margin-right: 0.8em;
    max-width: 35px !important;
    min-width: 35px !important;
}

.browser-container {
    position: relative;
    width: 100%;
    max-width: 100%;
    height: calc(100% - 140px);
}

.browser-container > div {
    height: 100%;
}

.progress-row {
    margin-top: 1em;
}

.progress-row .col-1 {
    min-width: 60px;
}

.progress-row .col-10 {
    margin: auto 0 auto 0;
}

.progress {
    padding: 0;
}
</style>
