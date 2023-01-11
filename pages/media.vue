<template>
    <div>
        <Menu />
        <div class="container-fluid browser-container">
            <Browser ref="browser" />
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
                                        <div class="col-10 progress">
                                            <div
                                                class="progress-bar bg-warning"
                                                role="progressbar"
                                                :aria-valuenow="overallProgress"
                                                :style="`width: ${overallProgress}%`"
                                            />
                                        </div>
                                        <div class="w-100" />
                                        <div class="col-1" style="min-width: 125px">Current:</div>
                                        <div class="col-10 progress">
                                            <div
                                                class="progress-bar bg-warning"
                                                role="progressbar"
                                                :aria-valuenow="currentProgress"
                                                :style="`width: ${currentProgress}%`"
                                            />
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
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'
import { useMedia } from '~/stores/media'
import Browser from '../components/Browser.vue'

const { $bootstrap } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

definePageMeta({
    middleware: ['auth'],
})

useHead({
    title: 'Media | ffplayout'
})

const browser = ref()
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
})

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

    await $fetch(`api/file/${configStore.configGui[configStore.configID].id}/create-folder/`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body: JSON.stringify({ source: path }),
    })
        .then(() => {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Folder create done...'
        })
        .catch((e) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = `Folder create error: ${e}`
        })

    indexStore.showAlert = true
    folderName.value = ''

    setTimeout(() => {
        indexStore.alertMsg = ''
        indexStore.showAlert = false
    }, 2000)

    browser.value.getPath(lastPath.value)
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
            `api/file/${configStore.configGui[configStore.configID].id}/upload/?path=${encodeURIComponent(
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
    browser.value.getPath(lastPath.value)

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
</style>
