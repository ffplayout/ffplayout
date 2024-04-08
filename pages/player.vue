<template>
    <div class="h-full">
        <Control />
        <div class="flex justify-end p-1">
            <div>
                <input type="date" class="input input-sm input-bordered w-full max-w-xs" v-model="listDate" />
            </div>
        </div>
        <div class="p-1 min-h-[500px] h-[calc(100vh-800px)] xl:h-[calc(100vh-480px)]">
            <splitpanes class="border border-my-gray rounded">
                <pane class="h-full" min-size="0" max-size="80" size="20">
                    <div
                        v-if="mediaStore.isLoading"
                        class="w-full h-full absolute z-10 flex justify-center bg-base-100/70"
                    >
                        <span class="loading loading-spinner loading-lg" />
                    </div>
                    <div class="bg-base-100 border-b border-my-gray">
                        <div v-if="mediaStore.folderTree.parent && mediaStore.crumbs">
                            <nav class="breadcrumbs px-3">
                                <ul>
                                    <li v-for="(crumb, index) in mediaStore.crumbs" :key="index">
                                        <button
                                            v-if="mediaStore.crumbs.length > 1 && mediaStore.crumbs.length - 1 > index"
                                            @click="mediaStore.getTree(crumb.path)"
                                        >
                                            <i class="bi-folder-fill me-1" />
                                            {{ crumb.text }}
                                        </button>
                                        <span v-else><i class="bi-folder-fill me-1" />{{ crumb.text }}</span>
                                    </li>
                                </ul>
                            </nav>
                        </div>
                    </div>

                    <ul class="h-[calc(100%-40px)] overflow-auto m-1">
                        <li class="flex px-1" v-for="folder in mediaStore.folderTree.folders" :key="folder.uid">
                            <button
                                class="truncate"
                                @click="mediaStore.getTree(`/${mediaStore.folderTree.source}/${folder.name}`)"
                            >
                                <i class="bi-folder-fill" />
                                {{ folder.name }}
                            </button>
                        </li>
                        <Sortable :list="mediaStore.folderTree.files" :options="browserSortOptions" item-key="name">
                            <template #item="{ element, index }">
                                <li
                                    :id="`file_${index}`"
                                    class="draggable px-1 grid grid-cols-[auto_110px]"
                                    :key="element.name"
                                >
                                    <div class="truncate cursor-grab">
                                        <i v-if="mediaType(element.name) === 'audio'" class="bi-music-note-beamed" />
                                        <i v-else-if="mediaType(element.name) === 'video'" class="bi-film" />
                                        <i
                                            v-else-if="mediaType(element.name) === 'image'"
                                            class="bi-file-earmark-image"
                                        />
                                        <i v-else class="bi-file-binary" />

                                        {{ element.name }}
                                    </div>
                                    <div>
                                        <button
                                            class="w-7"
                                            @click=";(showPreviewModal = true), setPreviewData(element.name)"
                                        >
                                            <i class="bi-play-fill" />
                                        </button>
                                        <div class="inline-block w-[82px]">{{ toMin(element.duration) }}</div>
                                    </div>
                                </li>
                            </template>
                        </Sortable>
                    </ul>
                </pane>
                <pane>
                    <div class="w-full h-full">
                        <div
                            class="grid grid-cols-[70px_auto_50px_70px_70px_70px_30px_60px_80px] bg-base-100 py-2 px-3 border-b border-my-gray"
                        >
                            <div>Start</div>
                            <div>File</div>
                            <div class="text-center">Play</div>
                            <div class="">Duration</div>
                            <div class="hidden md:flex">In</div>
                            <div class="hidden md:flex">Out</div>
                            <div class="hidden md:flex justify-center">Ad</div>
                            <div class="text-center">Edit</div>
                            <div class="hidden md:flex justify-center">Delete</div>
                        </div>
                        <div
                            v-if="playlistIsLoading"
                            class="w-full h-full absolute z-10 flex justify-center bg-base-100/70"
                        >
                            <span class="loading loading-spinner loading-lg" />
                        </div>
                        <div id="scroll-container" class="h-full overflow-auto">
                            <Sortable
                                :list="playlistStore.playlist"
                                item-key="uid"
                                class=""
                                :style="`height: ${
                                    playlistStore.playlist ? playlistStore.playlist.length * 38 + 76 : 300
                                }px`"
                                tag="ul"
                                :options="playlistSortOptions"
                                @add="cloneClip"
                                @end="moveItemInArray"
                            >
                                <template #item="{ element, index }">
                                    <li
                                        :id="`clip_${index}`"
                                        class="draggable bg-base-300 even:bg-base-100 grid grid-cols-[70px_auto_50px_70px_70px_70px_30px_60px_80px] h-[38px] px-3 py-[8px]"
                                        :class="
                                            index === playlistStore.currentClipIndex && listDate === todayDate
                                                ? 'active-playlist-clip'
                                                : ''
                                        "
                                        :key="element.uid"
                                    >
                                        <div>{{ secondsToTime(element.begin) }}</div>
                                        <div class="grabbing truncate cursor-grab">{{ filename(element.source) }}</div>
                                        <div class="text-center">
                                            <button @click=";(showPreviewModal = true), setPreviewData(element.source)">
                                                <i class="bi-play-fill" />
                                            </button>
                                        </div>
                                        <div>{{ secToHMS(element.duration) }}</div>
                                        <div class="hidden md:flex">{{ secToHMS(element.in) }}</div>
                                        <div class="hidden md:flex">{{ secToHMS(element.out) }}</div>
                                        <div class="hidden md:flex justify-center pt-[3px]">
                                            <input
                                                class="checkbox checkbox-xs rounded"
                                                type="checkbox"
                                                :checked="
                                                    element.category && element.category === 'advertisement'
                                                        ? true
                                                        : false
                                                "
                                                @change="setCategory($event, element)"
                                            />
                                        </div>
                                        <div class="text-center">
                                            <button @click=";(showSourceModal = true), editPlaylistItem(index)">
                                                <i class="bi-pencil-square" />
                                            </button>
                                        </div>
                                        <div class="text-center hidden md:flex justify-center">
                                            <button @click="deletePlaylistItem(index)">
                                                <i class="bi-x-circle-fill" />
                                            </button>
                                        </div>
                                    </li>
                                </template>
                            </Sortable>
                        </div>
                    </div>
                </pane>
            </splitpanes>
        </div>

        <div class="join flex justify-end m-3">
            <button class="btn btn-sm btn-primary join-item" title="Copy Playlist" @click="showCopyModal = true">
                <i class="bi-files" />
            </button>
            <button
                v-if="!configStore.configPlayout.playlist.loop"
                class="btn btn-sm btn-primary join-item"
                title="Loop Clips in Playlist"
                @click="loopClips()"
            >
                <i class="bi-view-stacked" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                title="Add (remote) Source to Playlist"
                @click="showSourceModal = true"
            >
                <i class="bi-file-earmark-plus" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                title="Import text/m3u file"
                @click="showImportModal = true"
            >
                <i class="bi-file-text" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                @click="mediaStore.getTree('', true), (showPlaylistGenerator = true)"
            >
                <i class="bi-sort-down-alt" />
            </button>
            <button class="btn btn-sm btn-primary join-item" title="Reset Playlist" @click="getPlaylist()">
                <i class="bi-arrow-counterclockwise" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                title="Save Playlist"
                @click=";(targetDate = listDate), savePlaylist(true)"
            >
                <i class="bi-download" />
            </button>
            <button class="btn btn-sm btn-primary join-item" title="Delete Playlist" @click="showDeleteModal = true">
                <i class="bi-trash" />
            </button>
        </div>

        <Modal :show="showPreviewModal" :title="`Preview: ${previewName}`" :modal-action="closePlayer">
            <div class="w-[1024px] max-w-full aspect-video">
                <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
                <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
            </div>
        </Modal>

        <Modal :show="showSourceModal" title="Add/Edit Source" :modal-action="processSource">
            <div>
                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">In</span>
                    </div>
                    <input type="number" class="input input-sm input-bordered w-full" v-model.number="newSource.in" />
                </label>

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">Out</span>
                    </div>
                    <input type="number" class="input input-sm input-bordered w-full" v-model.number="newSource.out" />
                </label>

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">Duration</span>
                    </div>
                    <input
                        type="number"
                        class="input input-sm input-bordered w-full"
                        v-model.number="newSource.duration"
                    />
                </label>

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">Source</span>
                    </div>
                    <input type="text" class="input input-sm input-bordered w-full" v-model="newSource.source" />
                </label>

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">Audio</span>
                    </div>
                    <input type="text" class="input input-sm input-bordered w-full" v-model="newSource.audio" />
                </label>

                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">Custom Filter</span>
                    </div>
                    <input type="text" class="input input-sm input-bordered w-full" v-model="newSource.custom_filter" />
                </label>

                <div class="form-control">
                    <label class="cursor-pointer label">
                        <span class="label-text">Advertisement</span>
                        <input type="checkbox" class="checkbox checkbox-sm" @click="isAd" />
                    </label>
                </div>
            </div>
        </Modal>

        <Modal :show="showImportModal" title="Import Playlist" :modal-action="importPlaylist">
            <input
                ref="fileImport"
                type="file"
                class="file-input file-input-sm file-input-bordered w-full"
                v-on:change="onFileChange"
                multiple
            />
        </Modal>

        <Modal :show="showCopyModal" :title="`Copy Program ${listDate}`" :modal-action="savePlaylist">
            <input type="date" class="input input-sm input-bordered w-full" v-model="targetDate" />
        </Modal>

        <Modal :show="showDeleteModal" title="Delete Program" :modal-action="deletePlaylist">
            <span>
                Delete program from <strong>{{ listDate }}</strong>
            </span>
        </Modal>

        <PlaylistGenerator v-if="showPlaylistGenerator" />
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const { $_, $dayjs } = useNuxtApp()
const { secToHMS, filename, secondsToTime, toMin, mediaType } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

useHead({
    title: 'Player | ffplayout',
})

const { configID } = storeToRefs(useConfig())
const { listDate } = storeToRefs(usePlaylist())

const fileImport = ref()
const playlistIsLoading = ref(false)
const todayDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const targetDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const editId = ref(-1)
const textFile = ref()

const showPreviewModal = ref(false)
const showSourceModal = ref(false)
const showImportModal = ref(false)
const showCopyModal = ref(false)
const showDeleteModal = ref(false)
const showPlaylistGenerator = ref(false)

const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)

const browserSortOptions = {
    group: { name: 'playlist', pull: 'clone', put: false },
    sort: false,
}
const playlistSortOptions = {
    group: 'playlist',
    animation: 100,
    handle: '.grabbing',
}

const newSource = ref({
    begin: 0,
    in: 0,
    out: 0,
    duration: 0,
    category: '',
    custom_filter: '',
    source: '',
    audio: '',
    uid: '',
} as PlaylistItem)

onMounted(async () => {
    if (!mediaStore.folderTree.parent) {
        await mediaStore.getTree('')
    }

    getPlaylist()
})

watch([listDate, configID], async () => {
    mediaStore.getTree('')
    await getPlaylist()
})

function scrollTo(index: number) {
    const child = document.getElementById(`clip_${index}`)
    const parent = document.getElementById('scroll-container')

    if (child && parent) {
        const topPos = child.offsetTop
        parent.scrollTop = topPos - 50
    }
}

async function getPlaylist() {
    playlistIsLoading.value = true
    await playlistStore.getPlaylist(listDate.value)
    playlistIsLoading.value = false

    if (listDate.value === todayDate.value) {
        scrollTo(playlistStore.currentClipIndex)
    } else {
        scrollTo(0)
    }
}

function closePlayer() {
    showPreviewModal.value = false
    isVideo.value = false
}

function setCategory(event: any, item: PlaylistItem) {
    if (event.target.checked) {
        item.category = 'advertisement'
    } else {
        item.category = ''
    }
}
function onFileChange(evt: any) {
    const files = evt.target.files || evt.dataTransfer.files

    if (!files.length) {
        return
    }

    textFile.value = files
}

function cloneClip(event: any) {
    const o = event.oldIndex
    const n = event.newIndex

    event.item.remove()

    const storagePath = configStore.configPlayout.storage.path
    const sourcePath = `${storagePath}/${mediaStore.folderTree.source}/${mediaStore.folderTree.files[o].name}`.replace(
        /\/[/]+/g,
        '/'
    )

    playlistStore.playlist.splice(n, 0, {
        uid: genUID(),
        begin: 0,
        source: sourcePath,
        in: 0,
        out: mediaStore.folderTree.files[o].duration,
        duration: mediaStore.folderTree.files[o].duration,
    })

    playlistStore.playlist = processPlaylist(
        configStore.startInSec,
        configStore.playlistLength,
        playlistStore.playlist,
        false
    )
}

function moveItemInArray(event: any) {
    playlistStore.playlist.splice(event.newIndex, 0, playlistStore.playlist.splice(event.oldIndex, 1)[0])

    playlistStore.playlist = processPlaylist(
        configStore.startInSec,
        configStore.playlistLength,
        playlistStore.playlist,
        false
    )
}

function setPreviewData(path: string) {
    let fullPath = path
    const storagePath = configStore.configPlayout.storage.path
    const lastIndex = storagePath.lastIndexOf('/')

    if (!path.includes('/')) {
        const parent = mediaStore.folderTree.parent ? mediaStore.folderTree.parent : ''
        fullPath = `/${parent}/${mediaStore.folderTree.source}/${path}`.replace(/\/[/]+/g, '/')
    } else if (lastIndex !== -1) {
        let pathPrefix = storagePath.substring(0, lastIndex)

        fullPath = path.replace(pathPrefix, '')
    }

    previewName.value = fullPath.split('/').slice(-1)[0]

    if (path.match(/^http/)) {
        previewUrl.value = path
    } else {
        previewUrl.value = encodeURIComponent(
            `/file/${configStore.configGui[configStore.configID].id}${fullPath}`
        ).replace(/%2F/g, '/')
    }

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

function processSource(process: boolean) {
    showSourceModal.value = false

    if (process) {
        if (editId.value === -1) {
            playlistStore.playlist.push(newSource.value)
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                playlistStore.playlist,
                false
            )
        } else {
            playlistStore.playlist[editId.value] = newSource.value
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                playlistStore.playlist,
                false
            )
        }
    }

    editId.value = -1
    newSource.value = {
        begin: 0,
        in: 0,
        out: 0,
        duration: 0,
        category: '',
        custom_filter: '',
        source: '',
        audio: '',
        uid: genUID(),
    }
}

function editPlaylistItem(i: number) {
    editId.value = i

    newSource.value = {
        begin: playlistStore.playlist[i].begin,
        in: playlistStore.playlist[i].in,
        out: playlistStore.playlist[i].out,
        duration: playlistStore.playlist[i].duration,
        category: playlistStore.playlist[i].category,
        custom_filter: playlistStore.playlist[i].custom_filter,
        source: playlistStore.playlist[i].source,
        audio: playlistStore.playlist[i].audio,
        uid: playlistStore.playlist[i].uid,
    }
}

function isAd(evt: any) {
    if (evt.target.checked) {
        newSource.value.category = 'advertisement'
    } else {
        newSource.value.category = ''
    }
}

function deletePlaylistItem(index: number) {
    playlistStore.playlist.splice(index, 1)
}

function loopClips() {
    const tempList = []
    let length = 0

    while (length < configStore.playlistLength && playlistStore.playlist.length > 0) {
        for (const item of playlistStore.playlist) {
            if (length < configStore.playlistLength) {
                tempList.push($_.cloneDeep(item))
                length += item.out - item.in
            } else {
                break
            }
        }
    }

    playlistStore.playlist = processPlaylist(configStore.startInSec, configStore.playlistLength, tempList, false)
}

async function importPlaylist(imp: boolean) {
    showImportModal.value = false

    if (imp) {
        if (!textFile.value || !textFile.value[0]) {
            return
        }

        const formData = new FormData()
        formData.append(textFile.value[0].name, textFile.value[0])

        playlistIsLoading.value = true
        await $fetch(
            `/api/file/${configStore.configGui[configStore.configID].id}/import/?file=${
                textFile.value[0].name
            }&date=${listDate}`,
            {
                method: 'PUT',
                headers: authStore.authHeader,
                body: formData,
            }
        )
            .then(() => {
                indexStore.msgAlert('alert-success', 'Import success!', 2)
                playlistStore.getPlaylist(listDate.value)
            })
            .catch((e: string) => {
                indexStore.msgAlert('alert-error', e, 4)
            })
    }

    playlistIsLoading.value = false
    textFile.value = null
    fileImport.value.value = null
}

async function savePlaylist(save: boolean) {
    showCopyModal.value = false

    if (save) {
        if (playlistStore.playlist.length === 0) {
            return
        }

        playlistStore.playlist = processPlaylist(
            configStore.startInSec,
            configStore.playlistLength,
            playlistStore.playlist,
            true
        )
        const saveList = playlistStore.playlist.map(({ begin, ...item }) => item)

        await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/`, {
            method: 'POST',
            headers: { ...contentType, ...authStore.authHeader },
            body: JSON.stringify({
                channel: configStore.configGui[configStore.configID].name,
                date: targetDate.value,
                program: saveList,
            }),
        })
            .then((response: any) => {
                indexStore.msgAlert('alert-success', response, 2)
            })
            .catch((e: any) => {
                if (e.status === 409) {
                    indexStore.msgAlert('alert-warning', e.data, 2)
                } else {
                    indexStore.msgAlert('alert-error', e, 4)
                }
            })
    }
}

async function deletePlaylist(del: boolean) {
    showDeleteModal.value = false

    if (del) {
        await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/${listDate}`, {
            method: 'DELETE',
            headers: { ...contentType, ...authStore.authHeader },
        }).then(() => {
            playlistStore.playlist = []

            indexStore.msgAlert('alert-warning', 'Playlist deleted...', 2)
        })
    }
}
</script>

<style lang="scss" scoped>
.filename,
.browser-item {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
}

.loading-overlay {
    width: 100%;
    height: 100%;
}

.player-container {
    position: relative;
    width: 100%;
    max-width: 100%;
    height: calc(100% - 140px);
}

.playlist-container {
    height: 100%;
    border: 1px solid $border-color;
    border-top: none;
    border-left: none;
    border-radius: $b-radius;
}

.player-container .media-browser-scroll {
    height: calc(100% - 39px);
}

.active-playlist-clip {
    background-color: #565e6a !important;
}

.list-row {
    height: calc(100% - 480px);
    min-height: 300px;
}

.pane-row {
    margin: 0;
}

.playlist-container {
    width: 100%;
    height: 100%;
}

.timecode {
    min-width: 65px;
    max-width: 90px;
}

.playlist-input {
    min-width: 42px;
    max-width: 60px;
}

.playlist-list-group,
#playlist-group {
    height: 100%;
}

.playlist-item {
    height: 38px;
}

.playlist-item:nth-of-type(odd) {
    background-color: #3b424a;
}

.playlist-item:hover {
    background-color: #1c1e22;
}

.overLength {
    background-color: #ed890641 !important;
}

#generateModal .modal-body {
    height: 600px;
}

.browser-col,
.template-col {
    height: 532px;
}

#generateModal {
    --bs-modal-width: 800px;
}

#generateModal .media-browser-scroll {
    height: calc(100% - 35px);
}

#generateModal .browser-div li:nth-of-type(odd) {
    background-color: #3b424a;
}

.select-all-div {
    margin-right: 20px;
}

.active-playlist-clip {
    background-color: #405f51 !important;
}
</style>
<style>
@media (max-width: 575px) {
    .mobile-hidden {
        display: none;
    }

    /*.splitpanes__splitter {
        display: none !important;
    }*/

    .playlist-pane {
        width: 100% !important;
    }
}
</style>
