<template>
    <div>
        <Menu />
        <Control />
        <div class="date-row">
            <div class="col">
                <input type="date" class="form-control date-div mt-2 mb-2" v-model="listDate" />
            </div>
        </div>
        <splitpanes class="container list-row pane-row player-container">
            <pane class="mobile-hidden" min-size="14" max-size="80" size="20">
                <div v-if="browserIsLoading" class="d-flex justify-content-center loading-overlay">
                    <div class="spinner-border" role="status" />
                </div>
                <div v-if="mediaStore.folderTree.parent && mediaStore.crumbs">
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
                <ul class="list-group media-browser-scroll browser-div">
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
                        </div>
                    </li>
                    <Sortable :list="mediaStore.folderTree.files" :options="browserSortOptions" item-key="name">
                        <template #item="{ element, index }">
                            <li
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
                                </div>
                            </li>
                        </template>
                    </Sortable>
                </ul>
            </pane>
            <pane class="playlist-pane">
                <div class="playlist-container">
                    <ul class="list-group list-group-header">
                        <li class="list-group-item">
                            <div class="row playlist-row">
                                <div class="col-1 timecode">Start</div>
                                <div class="col">File</div>
                                <div class="col-1 text-center playlist-input">Play</div>
                                <div class="col-1 timecode">Duration</div>
                                <div class="col-1 timecode mobile-hidden">In</div>
                                <div class="col-1 timecode mobile-hidden">Out</div>
                                <div class="col-1 text-center playlist-input mobile-hidden">Ad</div>
                                <div class="col-1 text-center playlist-input">Edit</div>
                                <div class="col-1 text-center playlist-input mobile-hidden">Delete</div>
                            </div>
                        </li>
                    </ul>
                    <div v-if="playlistIsLoading" class="d-flex justify-content-center loading-overlay">
                        <div class="spinner-border" role="status" />
                    </div>
                    <div id="scroll-container">
                        <Sortable
                            :list="playlistStore.playlist"
                            item-key="uid"
                            class="list-group playlist-list-group"
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
                                    class="draggable list-group-item playlist-item"
                                    :key="element.uid"
                                >
                                    <div class="row playlist-row">
                                        <div class="col-1 timecode">{{ secondsToTime(element.begin) }}</div>
                                        <div class="col grabbing filename">{{ filename(element.source) }}</div>
                                        <div class="col-1 text-center playlist-input">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#previewModal"
                                                @click="setPreviewData(element.source)"
                                            >
                                                <i class="bi-play-fill" />
                                            </a>
                                        </div>
                                        <div class="col-1 timecode">{{ secToHMS(element.duration) }}</div>
                                        <div class="col-1 timecode mobile-hidden">{{ secToHMS(element.in) }}</div>
                                        <div class="col-1 timecode mobile-hidden">{{ secToHMS(element.out) }}</div>
                                        <div class="col-1 text-center playlist-input mobile-hidden">
                                            <input
                                                class="form-check-input"
                                                type="checkbox"
                                                :checked="
                                                    element.category && element.category === 'advertisement'
                                                        ? true
                                                        : false
                                                "
                                                @change="setCategory($event, element)"
                                            />
                                        </div>
                                        <div class="col-1 text-center playlist-input">
                                            <a
                                                href="#"
                                                class="btn-link"
                                                data-bs-toggle="modal"
                                                data-bs-target="#sourceModal"
                                                @click="editPlaylistItem(index)"
                                            >
                                                <i class="bi-pencil-square" />
                                            </a>
                                        </div>
                                        <div class="col-1 text-center playlist-input mobile-hidden">
                                            <a href="#" class="btn-link" @click="deletePlaylistItem(index)">
                                                <i class="bi-x-circle-fill" />
                                            </a>
                                        </div>
                                    </div>
                                </li>
                            </template>
                        </Sortable>
                    </div>
                </div>
            </pane>
        </splitpanes>

        <div class="btn-group media-button mb-3">
            <div class="btn btn-primary" title="Copy Playlist" data-bs-toggle="modal" data-bs-target="#copyModal">
                <i class="bi-files" />
            </div>
            <div
                v-if="!configStore.configPlayout.playlist.loop"
                class="btn btn-primary"
                title="Loop Clips in Playlist"
                @click="loopClips()"
            >
                <i class="bi-view-stacked" />
            </div>
            <div
                class="btn btn-primary"
                title="Add (remote) Source to Playlist"
                data-bs-toggle="modal"
                data-bs-target="#sourceModal"
                @click="clearNewSource()"
            >
                <i class="bi-file-earmark-plus" />
            </div>
            <div
                class="btn btn-primary"
                title="Import text/m3u file"
                data-bs-toggle="modal"
                data-bs-target="#importModal"
            >
                <i class="bi-file-text" />
            </div>
            <div
                class="btn btn-primary"
                title="Generate a randomized Playlist"
                data-bs-toggle="modal"
                data-bs-target="#generateModal"
                @click="mediaStore.getTree('', true)"
            >
                <i class="bi-sort-down-alt" />
            </div>
            <div class="btn btn-primary" title="Reset Playlist" @click="getPlaylist()">
                <i class="bi-arrow-counterclockwise" />
            </div>
            <div class="btn btn-primary" title="Save Playlist" @click="savePlaylist(listDate)">
                <i class="bi-download" />
            </div>
            <div class="btn btn-primary" title="Delete Playlist" data-bs-toggle="modal" data-bs-target="#deleteModal">
                <i class="bi-trash" />
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

        <div id="sourceModal" class="modal" tabindex="-1" aria-labelledby="sourceModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="sourceModalLabel">Add/Edit Source</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <form @reset="clearNewSource">
                        <div class="modal-body">
                            <label for="in-input" class="form-label">In</label>
                            <input
                                type="number"
                                class="form-control"
                                id="in-input"
                                aria-describedby="in"
                                v-model.number="newSource.in"
                            />
                            <label for="out-input" class="form-label mt-2">Out</label>
                            <input
                                type="number"
                                class="form-control"
                                id="out-input"
                                aria-describedby="out"
                                v-model.number="newSource.out"
                            />
                            <label for="duration-input" class="form-label mt-2">Duration</label>
                            <input
                                type="number"
                                class="form-control"
                                id="duration-input"
                                aria-describedby="out"
                                v-model.number="newSource.duration"
                            />
                            <label for="source-input" class="form-label mt-2">Source</label>
                            <input
                                type="text"
                                class="form-control"
                                id="source-input"
                                aria-describedby="out"
                                v-model="newSource.source"
                            />
                            <label for="audio-input" class="form-label mt-2">Audio</label>
                            <input
                                type="text"
                                class="form-control"
                                id="audio-input"
                                aria-describedby="out"
                                v-model="newSource.audio"
                            />
                            <label for="filter-input" class="form-label mt-2">Custom Filter</label>
                            <input
                                type="text"
                                class="form-control"
                                id="filter-input"
                                aria-describedby="out"
                                v-model="newSource.custom_filter"
                            />
                            <div class="form-check">
                                <label class="form-check-label" for="ad-input"> Advertisement </label>
                                <input class="form-check-input" type="checkbox" value="" id="ad-input" @click="isAd" />
                            </div>
                        </div>
                        <div class="modal-footer">
                            <button type="reset" class="btn btn-primary" data-bs-dismiss="modal" aria-label="Cancel">
                                Cancel
                            </button>
                            <button
                                type="submit"
                                class="btn btn-primary"
                                data-bs-dismiss="modal"
                                @click="processSource"
                            >
                                Ok
                            </button>
                        </div>
                    </form>
                </div>
            </div>
        </div>

        <div id="importModal" class="modal" tabindex="-1" aria-labelledby="importModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="importModalLabel">Import Playlist</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <form @submit.prevent="onSubmitImport">
                        <div class="modal-body">
                            <input class="form-control" ref="fileImport" type="file" v-on:change="onFileChange" />
                        </div>
                        <div class="modal-footer">
                            <button type="reset" class="btn btn-primary" data-bs-dismiss="modal" aria-label="Cancel">
                                Cancel
                            </button>
                            <button type="submit" class="btn btn-primary" data-bs-dismiss="modal">Import</button>
                        </div>
                    </form>
                </div>
            </div>
        </div>

        <div id="copyModal" class="modal" tabindex="-1" aria-labelledby="copyModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="copyModalLabel">Copy Program {{ listDate }} to:</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">
                        <input type="date" class="form-control centered" v-model="targetDate" />
                    </div>
                    <div class="modal-footer">
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">Cancel</button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-bs-dismiss="modal"
                            @click="savePlaylist(targetDate)"
                        >
                            Ok
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <div id="deleteModal" class="modal" tabindex="-1" aria-labelledby="deleteModalLabel" aria-hidden="true">
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="deleteModalLabel">Delete Program</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">
                        Delete program from <strong>{{ listDate }}</strong>
                    </div>
                    <div class="modal-footer">
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">Cancel</button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-bs-dismiss="modal"
                            @click="deletePlaylist(listDate)"
                        >
                            Ok
                        </button>
                    </div>
                </div>
            </div>
        </div>

        <div
            id="generateModal"
            class="modal modal-xl"
            tabindex="-1"
            aria-labelledby="generateModalLabel"
            aria-hidden="true"
        >
            <div class="modal-dialog modal-dialog-centered">
                <div class="modal-content">
                    <div class="modal-header">
                        <h1 class="modal-title fs-5" id="generateModalLabel">Generate Program</h1>
                        <button type="button" class="btn-close" data-bs-dismiss="modal" aria-label="Cancel"></button>
                    </div>
                    <div class="modal-body">
                        <div>
                            <nav aria-label="breadcrumb">
                                <ol class="breadcrumb">
                                    <li
                                        class="breadcrumb-item"
                                        v-for="(crumb, index) in mediaStore.folderCrumbs"
                                        :key="index"
                                        :active="index === mediaStore.folderCrumbs.length - 1"
                                        @click="mediaStore.getTree(crumb.path, true)"
                                    >
                                        <a
                                            v-if="
                                                mediaStore.folderCrumbs.length > 1 &&
                                                mediaStore.folderCrumbs.length - 1 > index
                                            "
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
                        <ul class="list-group media-browser-scroll browser-div">
                            <li
                                class="list-group-item browser-item"
                                v-for="folder in mediaStore.folderList.folders"
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
                                            @click="
                                                ;[
                                                    (selectedFolders = []),
                                                    mediaStore.getTree(
                                                        `/${mediaStore.folderList.source}/${folder}`.replace(
                                                            /\/[/]+/g,
                                                            '/'
                                                        ),
                                                        true
                                                    ),
                                                ]
                                            "
                                        >
                                            {{ folder }}
                                        </a>
                                    </div>
                                    <div v-if="!generateFromAll" class="col-1 text-center playlist-input">
                                        <input
                                            class="form-check-input"
                                            type="checkbox"
                                            @change="
                                                setSelectedFolder(
                                                    $event,
                                                    `/${mediaStore.folderList.source}/${folder}`.replace(/\/[/]+/g, '/')
                                                )
                                            "
                                        />
                                    </div>
                                </div>
                            </li>
                        </ul>
                    </div>
                    <div class="modal-footer">
                        <div class="form-check select-all-div">
                            <input id="checkAll" class="form-check-input" type="checkbox" v-model="generateFromAll" />
                            <label class="form-check-label" for="checkAll">All</label>
                        </div>
                        <button type="button" class="btn btn-primary" data-bs-dismiss="modal">Cancel</button>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-bs-dismiss="modal"
                            @click="generatePlaylist()"
                        >
                            Ok
                        </button>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { Splitpanes, Pane } from 'splitpanes'
import 'splitpanes/dist/splitpanes.css'

import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'
import { useMedia } from '~/stores/media'
import { usePlaylist } from '~/stores/playlist'

const { $_, $dayjs } = useNuxtApp()
const { secToHMS, filename, secondsToTime, toMin, mediaType } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

definePageMeta({
    middleware: ['auth'],
})

useHead({
    title: 'Player | ffplayout',
})

const fileImport = ref()
const browserIsLoading = ref(false)
const playlistIsLoading = ref(false)
const listDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const targetDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const editId = ref(-1)
const textFile = ref()
const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)
const configID = ref(configStore.configID)
const selectedFolders = ref([] as string[])
const generateFromAll =ref(false)
const browserSortOptions = ref({
    group: { name: 'playlist', pull: 'clone', put: false },
    sort: false,
})
const playlistSortOptions = ref({
    group: 'playlist',
    animation: 100,
    handle: '.grabbing',
})
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

onMounted(() => {
    if (!mediaStore.folderTree.parent) {
        getPath('')
    }

    getPlaylist()
})

watch([listDate, configID], () => {
    getPlaylist()
})

async function getPath(path: string) {
    browserIsLoading.value = true
    await mediaStore.getTree(path)
    browserIsLoading.value = false
}

async function getPlaylist() {
    playlistIsLoading.value = true
    await playlistStore.getPlaylist(listDate.value)
    playlistIsLoading.value = false
}

function closePlayer() {
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
    if (!path.includes('/')) {
        const parent = mediaStore.folderTree.parent ? mediaStore.folderTree.parent : ''
        fullPath = `/${parent}/${mediaStore.folderTree.source}/${path}`.replace(/\/[/]+/g, '/')
    }

    previewName.value = fullPath.split('/').slice(-1)[0]
    previewUrl.value = encodeURIComponent(`${fullPath}`).replace(/%2F/g, '/')

    const ext = previewName.value.split('.').slice(-1)[0].toLowerCase()

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
                    type: `video/${ext}`,
                    src: previewUrl.value,
                },
            ],
        }
    } else {
        isVideo.value = false
    }
}

function processSource(evt: any) {
    evt.preventDefault()

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

        editId.value = -1
    }

    newSource.value = {
        begin: 0,
        in: 0,
        out: 0,
        duration: 0,
        category: '',
        custom_filter: '',
        source: '',
        audio: '',
        uid: '',
    }
}

function clearNewSource() {
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

    while (length < configStore.playlistLength) {
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

async function onSubmitImport(evt: any) {
    evt.preventDefault()

    if (!textFile.value || !textFile.value[0]) {
        return
    }

    const formData = new FormData()
    formData.append(textFile.value[0].name, textFile.value[0])

    playlistIsLoading.value = true
    await $fetch(
        `/api/file/${configStore.configGui[configStore.configID].id}/import/?file=${textFile.value[0].name}&date=${
            listDate.value
        }`,
        {
            method: 'PUT',
            headers: authStore.authHeader,
            body: formData,
        }
    )
        .then(() => {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Import success!'
            indexStore.showAlert = true

            playlistStore.getPlaylist(listDate.value)

            setTimeout(() => {
                indexStore.showAlert = false
            }, 2000)
        })
        .catch((e: string) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = e
            indexStore.showAlert = true

            setTimeout(() => {
                indexStore.showAlert = false
            }, 4000)
        })
    playlistIsLoading.value = false

    textFile.value = null
    fileImport.value.value = null
}

async function generatePlaylist() {
    playlistIsLoading.value = true

    let payload = {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
    } as Payload

    if (selectedFolders.value.length > 0 && !generateFromAll.value) {
        payload.body = { paths: selectedFolders.value }
    }

    await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/generate/${listDate.value}`, payload)
        .then((response: any) => {
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                response.program,
                false
            )
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = 'Generate Playlist done...'
            indexStore.showAlert = true

            setTimeout(() => {
                indexStore.showAlert = false
            }, 2000)
        })
        .catch((e: any) => {
            indexStore.alertVariant = 'alert-danger'
            indexStore.alertMsg = e.data ? e.data : e
            indexStore.showAlert = true

            setTimeout(() => {
                indexStore.showAlert = false
            }, 4000)
        })

    playlistIsLoading.value = false
}

async function savePlaylist(saveDate: string) {
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
            date: saveDate,
            program: saveList,
        }),
    })
        .then((response: any) => {
            indexStore.alertVariant = 'alert-success'
            indexStore.alertMsg = response
            indexStore.showAlert = true

            setTimeout(() => {
                indexStore.showAlert = false
            }, 2000)
        })
        .catch((e: any) => {
            if (e.status === 409) {
                indexStore.alertVariant = 'alert-warning'
                indexStore.alertMsg = e.data
                indexStore.showAlert = true

                setTimeout(() => {
                    indexStore.showAlert = false
                }, 2000)
            } else {
                indexStore.alertVariant = 'alert-danger'
                indexStore.alertMsg = e
                indexStore.showAlert = true

                setTimeout(() => {
                    indexStore.showAlert = false
                }, 4000)
            }
        })
}

async function deletePlaylist(playlistDate: string) {
    await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/${playlistDate}`, {
        method: 'DELETE',
        headers: { ...contentType, ...authStore.authHeader },
    }).then(() => {
        playlistStore.playlist = []

        indexStore.alertVariant = 'alert-warning'
        indexStore.alertMsg = 'Playlist deleted...'
        indexStore.showAlert = true

        setTimeout(() => {
            indexStore.showAlert = false
        }, 2000)
    })
}

function setSelectedFolder(event: any, folder: string) {
    if (event.target.checked) {
        selectedFolders.value.push(folder)
    } else {
        const index = selectedFolders.value.indexOf(folder)

        if (index > -1) {
            selectedFolders.value.splice(index, 1)
        }
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

#scroll-container {
    height: calc(100% - 40px);
    overflow: auto;
    scrollbar-width: medium;
}
.active-playlist-clip {
    background-color: #565e6a !important;
}

.list-row {
    height: calc(100% - 487px);
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
    height: 500px;
}

#generateModal {
    --bs-modal-width: 600px;
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
</style>
<style>
@media (max-width: 575px) {
    .mobile-hidden {
        display: none;
    }

    .splitpanes__splitter {
        display: none !important;
    }

    .playlist-pane {
        width: 100% !important;
    }
}
</style>
