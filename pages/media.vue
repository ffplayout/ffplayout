<template>
    <div>
        <div class="h-[calc(100vh-140px)] px-2">
            <nav class="text-sm breadcrumbs px-3">
                <ul @dragover.prevent>
                    <li
                        v-for="(crumb, index) in mediaStore.crumbs"
                        :key="index"
                        @drop="handleDrop($event, crumb.path, null)"
                        @dragover="handleDragOver"
                        @dragleave="handleDragLeave"
                    >
                        <button
                            v-if="mediaStore.crumbs.length > 1 && mediaStore.crumbs.length - 1 > index"
                            @click="mediaStore.getTree(crumb.path)"
                        >
                            <i class="bi-folder-fill me-1" />
                            {{ crumb.text }}
                        </button>
                        <span v-else><i class="bi-folder-fill me-1" /> {{ crumb.text }}</span>
                    </li>
                </ul>
            </nav>

            <div class="relative h-[calc(100%-34px)] min-h-[300px] bg-base-100">
                <div v-if="mediaStore.isLoading" class="w-full h-full absolute z-10 flex justify-center bg-base-100/70">
                    <span class="loading loading-spinner loading-lg" />
                </div>
                <splitpanes :horizontal="horizontal" class="h-full border border-my-gray rounded shadow">
                    <pane
                        min-size="14"
                        max-size="80"
                        size="20"
                        class="h-full bg-base-300"
                        :class="horizontal ? 'rounded-t' : 'rounded-s'"
                    >
                        <div class="relative h-full overflow-y-auto">
                            <table
                                v-if="mediaStore.folderTree.parent"
                                class="table table-zebra table-fixed"
                                @dragover.prevent
                            >
                                <tbody>
                                    <template v-if="mediaStore.folderTree.parent_folders.length > 0">
                                        <tr
                                            v-for="folder in mediaStore.folderTree.parent_folders"
                                            :key="folder.uid"
                                            class="grid grid-cols-[auto_30px] border-b border-base-content/20"
                                            :class="
                                                filename(mediaStore.folderTree.source) === folder.name &&
                                                'bg-base-300 rounded'
                                            "
                                            @drop="handleDrop($event, folder, true)"
                                            @dragover="handleDragOver"
                                            @dragleave="handleDragLeave"
                                        >
                                            <td class="px-2 py-1.5 truncate">
                                                <span
                                                    class="cursor-pointer"
                                                    @click="
                                                        mediaStore.getTree(
                                                            `/${parent(mediaStore.folderTree.source)}/${folder.name}`
                                                        )
                                                    "
                                                >
                                                    <i class="bi-folder-fill" />
                                                    {{ folder.name }}
                                                </span>
                                            </td>

                                            <td class="px-2 py-1.5 text-center">
                                                <button
                                                    class="opacity-30 hover:opacity-100"
                                                    @click="
                                                        ;(showDeleteModal = true),
                                                            (deleteName = `/${parent(mediaStore.folderTree.source)}/${
                                                                folder.name
                                                            }`.replace(/\/[/]+/g, '/'))
                                                    "
                                                >
                                                    <i class="bi-x-circle-fill" />
                                                </button>
                                            </td>
                                        </tr>
                                    </template>

                                    <tr v-else>
                                        <td class="px-2 py-1.5 border-b border-base-content/20">
                                            <i class="bi-folder-fill" />
                                            {{ mediaStore.folderTree.parent }}
                                        </td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </pane>
                    <pane class="h-full !bg-base-300" :class="horizontal ? 'rounded-b' : 'rounded-e'">
                        <div class="relative h-full overflow-y-auto">
                            <table
                                v-if="mediaStore.folderTree.parent"
                                class="table table-zebra table-fixed"
                                @dragover.prevent
                            >
                                <tbody>
                                    <tr
                                        v-for="folder in mediaStore.folderTree.folders"
                                        :key="folder.uid"
                                        class="grid grid-cols-[auto_170px] border-b border-base-content/20"
                                        @drop="handleDrop($event, folder, false)"
                                        @dragover="handleDragOver"
                                        @dragleave="handleDragLeave"
                                    >
                                        <td class="px-2 py-1.5">
                                            <button
                                                class="truncate"
                                                @click="
                                                    mediaStore.getTree(
                                                        `/${mediaStore.folderTree.source}/${folder.name}`
                                                    )
                                                "
                                            >
                                                <i class="bi-folder-fill" />
                                                {{ folder.name }}
                                            </button>
                                        </td>
                                        <td class="ps-2 pe-[14px] py-1.5 text-right">
                                            <button
                                                class="opacity-30 hover:opacity-100"
                                                @click="
                                                    ;(showDeleteModal = true),
                                                        (deleteName =
                                                            `/${mediaStore.folderTree.source}/${folder.name}`.replace(
                                                                /\/[/]+/g,
                                                                '/'
                                                            ))
                                                "
                                            >
                                                <i class="bi-x-circle-fill" />
                                            </button>
                                        </td>
                                    </tr>
                                    <tr
                                        v-for="(element, index) in mediaStore.folderTree.files"
                                        :id="`file_${index}`"
                                        :key="element.name"
                                        class="grid grid-cols-[auto_170px] border-b border-base-content/20"
                                        draggable="true"
                                        @dragstart="handleDragStart($event, element)"
                                    >
                                        <td class="px-2 py-1.5 truncate cursor-grab">
                                            <i
                                                v-if="mediaType(element.name) === 'audio'"
                                                class="bi-music-note-beamed"
                                            />
                                            <i v-else-if="mediaType(element.name) === 'video'" class="bi-film" />
                                            <i
                                                v-else-if="mediaType(element.name) === 'image'"
                                                class="bi-file-earmark-image"
                                            />
                                            <i v-else class="bi-file-binary" />

                                            {{ element.name }}
                                        </td>
                                        <td class="px-2 py-1.5 flex">
                                            <button
                                                class="w-7"
                                                @click=";(showPreviewModal = true), setPreviewData(element.name)"
                                            >
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
                                                @click="
                                                    ;(showDeleteModal = true),
                                                        (deleteName =
                                                            `/${mediaStore.folderTree.source}/${element.name}`.replace(
                                                                /\/[/]+/g,
                                                                '/'
                                                            ))
                                                "
                                            >
                                                <i class="bi-x-circle-fill" />
                                            </button>
                                        </td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </pane>
                </splitpanes>
            </div>

            <div class="flex justify-end py-4 pe-2">
                <div class="join">
                    <button
                        class="btn btn-sm btn-primary join-item"
                        :title="$t('media.create')"
                        @click="showCreateModal = true"
                    >
                        <i class="bi-folder-plus" />
                    </button>
                    <button
                        class="btn btn-sm btn-primary join-item"
                        :title="$t('media.upload')"
                        @click="showUploadModal = true"
                    >
                        <i class="bi-upload" />
                    </button>
                </div>
            </div>
        </div>

        <GenericModal
            :show="showDeleteModal"
            :title="$t('media.deleteTitle')"
            :text="`${$t('media.deleteQuestion')}:<br /><strong>${deleteName}</strong>`"
            :modal-action="deleteFileOrFolder"
        />

        <GenericModal
            :show="showPreviewModal"
            :title="`${$t('media.preview')}: ${previewName}`"
            :modal-action="closePlayer"
            :hide-buttons="true"
        >
            <div class="w-[1024px] max-w-full aspect-video">
                <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
                <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
            </div>
        </GenericModal>

        <GenericModal :show="showRenameModal" :title="$t('media.rename')" :modal-action="renameFile">
            <label class="form-control w-full max-w-md">
                <div class="label">
                    <span class="label-text">{{ $t('media.newFile') }}</span>
                </div>
                <input v-model="renameNewName" type="text" class="input input-bordered w-full" />
            </label>
        </GenericModal>

        <GenericModal :show="showCreateModal" :title="$t('media.createFolder')" :modal-action="createFolder">
            <label class="form-control w-full max-w-md">
                <div class="label">
                    <span class="label-text">{{ $t('media.foldername') }}</span>
                </div>
                <input v-model="folderName.name" type="text" class="input input-bordered w-full" />
            </label>
        </GenericModal>

        <GenericModal :show="showUploadModal" :title="$t('media.upload')" :modal-action="uploadFiles">
            <div class="w-[700px] max-w-full">
                <input
                    ref="fileInputName"
                    type="file"
                    class="file-input file-input-bordered w-full"
                    :accept="extensions"
                    multiple
                    @change="onFileChange"
                />

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">{{ $t('media.current') }}:</span>
                    </div>
                    <progress class="progress progress-accent" :value="currentProgress" max="100" />
                </label>

                <label class="form-control w-full mt-1">
                    <div class="label">
                        <span class="label-text"
                            >{{ $t('media.overall') }} ({{ currentNumber }}/{{ inputFiles.length }}):</span
                        >
                    </div>
                    <progress class="progress progress-accent" :value="overallProgress" max="100" />
                </label>
                <label class="form-control w-full mt-1">
                    <div class="label">
                        <span class="label-text">{{ $t('media.uploading') }}:</span>
                    </div>
                    <input v-model="uploadTask" type="text" class="input input-sm input-bordered w-full" disabled />
                </label>
            </div>
        </GenericModal>
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const { t } = useI18n()
const { width } = useWindowSize({ initialWidth: 800 })
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const { toMin, mediaType, filename, parent } = stringFormatter()
const { configID } = storeToRefs(useConfig())

useHead({
    title: `${t('button.media')} | ffplayout`,
})

watch([width], () => {
    if (width.value < 640) {
        horizontal.value = true
    } else {
        horizontal.value = false
    }
})

const horizontal = ref(false)
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

onMounted(async () => {
    let config_extensions = configStore.playout.storage.extensions
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
        await mediaStore.getTree('')
    }
})

watch([configID], () => {
    mediaStore.getTree('')
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
    const source = `/${mediaStore.folderTree.source}/${itemData.name}`.replace(/\/[/]+/g, '/')
    let target

    if (isParent === null) {
        target = `${targetFolder}/${itemData.name}`.replace(/\/[/]+/g, '/')
    } else if (isParent) {
        target = `/${parent(mediaStore.folderTree.source)}/${targetFolder.name}/${itemData.name}`.replace(
            /\/[/]+/g,
            '/'
        )
    } else {
        target = `/${mediaStore.folderTree.source}/${targetFolder.name}/${itemData.name}`.replace(/\/[/]+/g, '/')
    }

    event.target.style.fontWeight = null

    if (event.target.firstChild.classList.contains('bi-folder2-open')) {
        event.target.firstChild.classList.remove('bi-folder2-open')
        event.target.firstChild.classList.add('bi-folder-fill')
    }

    if (source !== target) {
        await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/rename/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source, target }),
        })
            .then(async (res) => {
                if (res.status >= 400) {
                    indexStore.msgAlert('error', await res.json(), 3)
                } else {
                    mediaStore.getTree(mediaStore.folderTree.source)
                }
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.moveError')}: ${e}`, 3)
            })
    }
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

    if (configStore.playout.storage.extensions.includes(`${ext}`)) {
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
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: deleteName.value }),
        })
            .then(async (response) => {
                if (response.status !== 200) {
                    indexStore.msgAlert('error', `${await response.text()}`, 5)
                }
                mediaStore.getTree(mediaStore.folderTree.source)
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.deleteError')}: ${e}`, 5)
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

    if (ren && renameOldName.value !== renameNewName.value) {
        await fetch(`/api/file/${configStore.configGui[configStore.configID].id}/rename/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: renameOldName.value, target: renameNewName.value }),
        })
            .then(async (res) => {
                if (res.status >= 400) {
                    indexStore.msgAlert('error', await res.text(), 3)
                } else {
                    mediaStore.getTree(mediaStore.folderTree.source)
                }
            })
            .catch((e) => {
                indexStore.msgAlert('error', `${t('media.moveError')}: ${e}`, 3)
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
            indexStore.msgAlert('warning', `${t('media.folderExists')}! "${folderName.value.name}"`, 2)

            return
        }

        await $fetch(`/api/file/${configStore.configGui[configStore.configID].id}/create-folder/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ source: path }),
        })
            .then(() => {
                indexStore.msgAlert('success', t('media.folderCreate'), 2)
            })
            .catch((e: string) => {
                indexStore.msgAlert('error', `${t('media.folderError')}: ${e}`, 3)
                indexStore.alertVariant = 'error'
            })

        mediaStore.getTree(lastPath.value)
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
            indexStore.msgAlert('error', `${t('media.folderError')}: ${xhr.value.status}`, 3)

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
                indexStore.msgAlert('warning', t('media.fileExists'), 3)
            } else {
                await upload(file)
            }

            overallProgress.value = (currentNumber.value * 100) / inputFiles.value.length
        }

        uploadTask.value = 'Done...'
        mediaStore.getTree(lastPath.value)

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
