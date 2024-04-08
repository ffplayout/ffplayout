<template>
    <div class="h-[calc(100vh-140px)] px-2">
        <nav class="text-sm breadcrumbs px-3">
            <ul v-on:dragover.prevent>
                <li
                    v-for="(crumb, index) in mediaStore.crumbs"
                    :key="index"
                    :active="index === mediaStore.crumbs.length - 1"
                    @click="getPath(crumb.path)"
                    v-on:drop="handleDrop($event, crumb.path, null)"
                    v-on:dragover="handleDragOver"
                    v-on:dragleave="handleDragLeave"
                >
                    <button v-if="mediaStore.crumbs.length > 1 && mediaStore.crumbs.length - 1 > index">
                        <i class="bi-folder-fill me-1" />
                        {{ crumb.text }}
                    </button>
                    <span v-else><i class="bi-folder-fill me-1" /> {{ crumb.text }}</span>
                </li>
            </ul>
        </nav>

        <div class=" h-[calc(100%-34px)] bg-base-100">
            <div
                v-if="browserIsLoading"
                class="w-[calc(100%-16px)] h-[calc(100%-174px)] absolute z-10 flex justify-center bg-base-100/70"
            >
                <span class="loading loading-spinner loading-lg"></span>
            </div>
            <splitpanes :horizontal="horizontal" class="border border-my-gray rounded">
                <pane min-size="14" max-size="80" size="24" class="h-full px-1 pb-1">
                    <ul v-if="mediaStore.folderTree.parent" class="overflow-auto h-full m-1" v-on:dragover.prevent>
                        <li
                            v-if="mediaStore.folderTree.parent_folders.length > 0"
                            v-for="folder in mediaStore.folderTree.parent_folders"
                            class="grid grid-cols-[auto_38px] gap-1 px-1"
                            :class="filename(mediaStore.folderTree.source) === folder.name && 'bg-base-300 rounded'"
                            :key="folder.uid"
                            v-on:drop="handleDrop($event, folder, true)"
                            v-on:dragover="handleDragOver"
                            v-on:dragleave="handleDragLeave"
                        >
                            <button
                                class="truncate text-left"
                                @click="getPath(`/${parent(mediaStore.folderTree.source)}/${folder.name}`)"
                            >
                                <i class="bi-folder-fill" />
                                {{ folder.name }}
                            </button>
                            <button
                                class="w-7 opacity-30 hover:opacity-100"
                                @click="
                                    ;(showDeleteModal = true),
                                        (deleteName = `/${parent(mediaStore.folderTree.source)}/${folder.name}`.replace(
                                            /\/[/]+/g,
                                            '/'
                                        ))
                                "
                            >
                                <i class="bi-x-circle-fill" />
                            </button>
                        </li>
                        <li v-else class="px-2">
                            <div class="truncate text-left">
                                <i class="bi-folder-fill" />
                                {{ mediaStore.folderTree.parent }}
                            </div>
                        </li>
                    </ul>
                </pane>
                <pane class="h-full px-2 pb-1">
                    <ul v-if="mediaStore.folderTree.parent" class="h-full overflow-auto m-1" v-on:dragover.prevent>
                        <li
                            class="grid grid-cols-[auto_49px] gap-1"
                            v-for="folder in mediaStore.folderTree.folders"
                            :key="folder.uid"
                            v-on:drop="handleDrop($event, folder, false)"
                            v-on:dragover="handleDragOver"
                            v-on:dragleave="handleDragLeave"
                        >
                            <button
                                class="truncate text-left"
                                @click="getPath(`/${mediaStore.folderTree.source}/${folder.name}`)"
                            >
                                <i class="bi-folder-fill" />
                                {{ folder.name }}
                            </button>
                            <button
                                class="opacity-30 hover:opacity-100"
                                @click="
                                    ;(showDeleteModal = true),
                                        (deleteName = `/${mediaStore.folderTree.source}/${folder.name}`.replace(
                                            /\/[/]+/g,
                                            '/'
                                        ))
                                "
                            >
                                <i class="bi-x-circle-fill" />
                            </button>
                        </li>
                        <li
                            v-for="(element, index) in mediaStore.folderTree.files"
                            :id="`file_${index}`"
                            class="grid grid-cols-[auto_176px]"
                            :key="element.name"
                            draggable="true"
                            v-on:dragstart="handleDragStart($event, element)"
                        >
                            <div class="truncate cursor-grab">
                                <i v-if="mediaType(element.name) === 'audio'" class="bi-music-note-beamed" />
                                <i v-else-if="mediaType(element.name) === 'video'" class="bi-film" />
                                <i v-else-if="mediaType(element.name) === 'image'" class="bi-file-earmark-image" />
                                <i v-else class="bi-file-binary" />

                                {{ element.name }}
                            </div>
                            <div>
                                <button class="w-7" @click=";(showPreviewModal = true), setPreviewData(element.name)">
                                    <i class="bi-play-fill" />
                                </button>

                                <div class="inline-block w-[82px]">{{ toMin(element.duration) }}</div>

                                <button
                                    class="w-7"
                                    @click="
                                        ;(showRenameModal = true),
                                            setRenameValues(
                                                `/${mediaStore.folderTree.source}/${element.name}`.replace(
                                                    /\/[/]+/g,
                                                    '/'
                                                )
                                            )
                                    "
                                >
                                    <i class="bi-pencil-square" />
                                </button>

                                <button
                                    class="w-7 opacity-30 hover:opacity-100"
                                    data-bs-toggle="modal"
                                    data-bs-target="#deleteModal"
                                    @click="
                                        ;(showDeleteModal = true),
                                            (deleteName = `/${mediaStore.folderTree.source}/${element.name}`.replace(
                                                /\/[/]+/g,
                                                '/'
                                            ))
                                    "
                                >
                                    <i class="bi-x-circle-fill" />
                                </button>
                            </div>
                        </li>
                    </ul>
                </pane>
            </splitpanes>
        </div>
    </div>
    <div class="flex justify-end pe-10 mt-7">
        <div class="join">
            <button class="btn btn-sm btn-primary join-item" title="Create Folder" @click="showCreateModal = true">
                <i class="bi-folder-plus" />
            </button>
            <button class="btn btn-sm btn-primary join-item" title="Upload File" @click="showUploadModal = true">
                <i class="bi-upload" />
            </button>
        </div>
    </div>

    <Modal
        :show="showDeleteModal"
        title="Delete File/Folder"
        :text="`Are you sure that you want to delete:<br /><strong>${deleteName}</strong>`"
        :modal-action="deleteFileOrFolder"
    />

    <Modal :show="showPreviewModal" :title="`Preview: ${previewName}`" :modal-action="closePlayer">
        <div class="w-[1024px] max-w-full aspect-video">
            <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
            <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
        </div>
    </Modal>

    <Modal :show="showRenameModal" title="Rename File" :modal-action="renameFile">
        <label class="form-control w-full max-w-md">
            <div class="label">
                <span class="label-text">New filename</span>
            </div>
            <input type="text" class="input input-bordered w-full" v-model="renameNewName" />
        </label>
    </Modal>

    <Modal :show="showCreateModal" title="Create Folder" :modal-action="createFolder">
        <label class="form-control w-full max-w-md">
            <div class="label">
                <span class="label-text">Foldername</span>
            </div>
            <input type="text" class="input input-bordered w-full" v-model="folderName.name" />
        </label>
    </Modal>

    <Modal :show="showUploadModal" title="Upload Files" :modal-action="uploadFiles">
        <div class="w-[700px] max-w-full">
            <input
                type="file"
                class="file-input file-input-bordered w-full"
                ref="fileInputName"
                :accept="extensions"
                v-on:change="onFileChange"
                multiple
            />

            <label class="form-control w-full mt-3">
                <div class="label">
                    <span class="label-text">Current:</span>
                </div>
                <progress class="progress progress-accent" :value="currentProgress" max="100" />
            </label>

            <label class="form-control w-full mt-1">
                <div class="label">
                    <span class="label-text">Overall ({{ currentNumber }}/{{ inputFiles.length }}):</span>
                </div>
                <progress class="progress progress-accent" :value="overallProgress" max="100" />
            </label>
            <label class="form-control w-full mt-1">
                <div class="label">
                    <span class="label-text">Uploading:</span>
                </div>
                <input type="text" class="input input-sm input-bordered w-full" v-model="uploadTask" disabled />
            </label>
        </div>
    </Modal>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const { width } = useWindowSize({ initialWidth: 800 })
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const { toMin, mediaType, filename, parent } = stringFormatter()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const { configID } = storeToRefs(useConfig())

useHead({
    title: 'Media | ffplayout',
})

watch([width], () => {
    if (width.value < 640) {
        horizontal.value = true
    } else {
        horizontal.value = false
    }
})

const horizontal = ref(false)
const browserIsLoading = ref(false)
const deleteName = ref('')
const renameOldName = ref('')
const renameNewName = ref('')
const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)
const showDeleteModal = ref(false)
const showPreviewModal = ref(false)
const showRenameModal = ref(false)
const showCreateModal = ref(false)
const showUploadModal = ref(false)
const extensions = ref('')
const folderName = ref({} as Folder)
const inputFiles = ref([] as File[])
const fileInputName = ref()
const currentNumber = ref(0)
const uploadTask = ref('')
const overallProgress = ref(0)
const currentProgress = ref(0)
const lastPath = ref('')
const xhr = ref(new XMLHttpRequest())

const fileRefs = ref([] as any[])

onMounted(async () => {
    let config_extensions = configStore.configPlayout.storage.extensions
    let extra_extensions = configStore.configGui[configStore.configID].extra_extensions

    if (typeof config_extensions === 'string') {
        config_extensions = config_extensions.split(',')
    }

    if (typeof extra_extensions === 'string') {
        extra_extensions = extra_extensions.split(',')
    }

    const exts = [...config_extensions, ...extra_extensions].map((ext) => {
        return `.${ext}`
    })

    extensions.value = exts.join(', ')

    if (!mediaStore.folderTree.parent) {
        await getPath('')
    }
})

watch([configID], () => {
    getPath('')
})

function handleDragStart(event: any, itemData: any) {
    event.dataTransfer.setData('application/json', JSON.stringify(itemData))
}

function handleDragOver(event: any) {
    event.target.style.fontWeight = 'bold'

    if (event.target.firstChild && event.target.firstChild.classList.contains('bi-folder-fill')) {
        event.target.firstChild.classList.remove('bi-folder-fill')
        event.target.firstChild.classList.add('bi-folder2-open')
    }
}

function handleDragLeave(event: any) {
    if (event.target && event.target.style) {
        event.target.style.fontWeight = null
    }

    if (event.target.firstChild && event.target.firstChild.classList.contains('bi-folder2-open')) {
        event.target.firstChild.classList.remove('bi-folder2-open')
        event.target.firstChild.classList.add('bi-folder-fill')
    }
}

async function handleDrop(event: any, targetFolder: any, isParent: boolean | null) {
    const itemData = JSON.parse(event.dataTransfer.getData('application/json'))
    const source = `/${mediaStore.folderTree.source}/${itemData.name}`.replace(
        /\/[/]+/g,
        '/'
    )
    let target

    if (isParent === null) {
        target = `${targetFolder}/${itemData.name}`.replace(/\/[/]+/g, '/')
    } else if (isParent) {
        target = `/${parent(mediaStore.folderTree.source)}/${targetFolder.name}/${
            itemData.name
        }`.replace(/\/[/]+/g, '/')
    } else {
        target =
            `/${mediaStore.folderTree.source}/${targetFolder.name}/${itemData.name}`.replace(
                /\/[/]+/g,
                '/'
            )
    }

    event.target.style.fontWeight = null

    if (event.target.firstChild.classList.contains('bi-folder2-open')) {
        event.target.firstChild.classList.remove('bi-folder2-open')
        event.target.firstChild.classList.add('bi-folder-fill')
    }

    if (source !== target) {
        await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/rename/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({ source, target }),
        })
            .then(() => {
                getPath(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('alert-error', `Delete error: ${e}`, 3)
            })
    }
}

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
    previewUrl.value = encodeURIComponent(`/file/${configStore.configGui[configStore.configID].id}${fullPath}`).replace(
        /%2F/g,
        '/'
    )

    const ext = previewName.value.split('.').slice(-1)[0].toLowerCase()
    const fileType =
        mediaType(previewName.value) === 'audio'
            ? `audio/${ext}`
            : mediaType(previewName.value) === 'live'
            ? 'application/x-mpegURL'
            : `video/${ext}`

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

async function deleteFileOrFolder(del: boolean) {
    /*
        Delete function, works for files and folders.
    */
    showDeleteModal.value = false

    if (del) {
        await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/remove/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: deleteName.value }),
        })
            .then(async (response) => {
                if (response.status !== 200) {
                    indexStore.msgAlert('alert-error', `${await response.text()}`, 5)
                }
                getPath(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('alert-error', `Delete error: ${e}`, 5)
            })
    }

    deleteName.value = ''
}

function setRenameValues(path: string) {
    renameNewName.value = path
    renameOldName.value = path
}

async function renameFile(ren: boolean) {
    /*
        Form submit for file rename request.
    */
    showRenameModal.value = false

    if (ren) {
        await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/rename/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: renameOldName.value, target: renameNewName.value }),
        })
            .then(() => {
                getPath(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('alert-error', `Delete error: ${e}`, 3)
            })
    }

    renameOldName.value = ''
    renameNewName.value = ''
}

function closePlayer() {
    showPreviewModal.value = false
    isVideo.value = false
}

async function createFolder(create: boolean) {
    showCreateModal.value = false

    if (create) {
        const path = `${mediaStore.folderTree.source}/${folderName.value.name}`.replace(/\/[/]+/g, '/')
        lastPath.value = mediaStore.folderTree.source

        if (mediaStore.folderTree.folders.includes(folderName.value)) {
            indexStore.msgAlert('alert-warning', `Folder "${folderName.value.name}" exists already!`, 2)

            return
        }

        await $fetch(`/api/file/${configStore.configGui[configStore.configID].id}/create-folder/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: path }),
        })
            .then(() => {
                indexStore.msgAlert('alert-success', 'Folder create done...', 2)
            })
            .catch((e: string) => {
                indexStore.msgAlert('alert-error', `Folder create error: ${e}`, 3)
                indexStore.alertVariant = 'alert-error'
            })

        getPath(lastPath.value)
    }

    folderName.value = {} as Folder
}

function onFileChange(evt: any) {
    const files = evt.target.files || evt.dataTransfer.files

    if (!files.length) {
        return
    }

    inputFiles.value = files
}

async function upload(file: any): Promise<null | undefined> {
    const formData = new FormData()
    formData.append(file.name, file)
    xhr.value = new XMLHttpRequest()

    return new Promise((resolve) => {
        xhr.value.open(
            'PUT',
            `/api/file/${configStore.configGui[configStore.configID].id}/upload/?path=${encodeURIComponent(
                mediaStore.crumbs[mediaStore.crumbs.length - 1].path
            )}`
        )

        xhr.value.setRequestHeader('Authorization', `Bearer ${authStore.jwtToken}`)

        xhr.value.upload.onprogress = (event: any) => {
            currentProgress.value = Math.round((100 * event.loaded) / event.total)
        }

        xhr.value.upload.onerror = () => {
            indexStore.msgAlert('alert-error', `Upload error: ${xhr.value.status}`, 3)

            resolve(undefined)
        }

        // upload completed successfully
        xhr.value.onload = () => {
            currentProgress.value = 100
            resolve(xhr.value.response)
        }

        xhr.value.send(formData)
    })
}

async function uploadFiles(upl: boolean) {
    if (upl) {
        authStore.inspectToken()
        lastPath.value = mediaStore.folderTree.source

        for (let i = 0; i < inputFiles.value.length; i++) {
            const file = inputFiles.value[i]
            uploadTask.value = file.name
            currentProgress.value = 0
            currentNumber.value = i + 1

            if (mediaStore.folderTree.files.find((f) => f.name === file.name)) {
                indexStore.msgAlert('alert-warning', 'File exists already!', 3)
            } else {
                await upload(file)
            }

            overallProgress.value = (currentNumber.value * 100) / inputFiles.value.length
        }

        uploadTask.value = 'Done...'
        getPath(lastPath.value)

        setTimeout(() => {
            fileInputName.value = null
            currentNumber.value = 0
            currentProgress.value = 0
            overallProgress.value = 0
            inputFiles.value = []
            uploadTask.value = ''
            showUploadModal.value = false
        }, 1500)
    } else {
        fileInputName.value = null
        inputFiles.value = []
        overallProgress.value = 0
        currentProgress.value = 0
        uploadTask.value = ''
        xhr.value.abort()
        showUploadModal.value = false
    }
}
</script>
