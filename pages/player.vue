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
            <button
                class="btn btn-sm btn-primary join-item"
                title="Copy Playlist"
                data-bs-toggle="modal"
                data-bs-target="#copyModal"
            >
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
                data-bs-toggle="modal"
                data-bs-target="#importModal"
            >
                <i class="bi-file-text" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                title="Generate a randomized Playlist"
                data-bs-toggle="modal"
                data-bs-target="#generateModal"
                @click="mediaStore.getTree('', true)"
            >
                <i class="bi-sort-down-alt" />
            </button>
            <button class="btn btn-sm btn-primary join-item" title="Reset Playlist" @click="getPlaylist()">
                <i class="bi-arrow-counterclockwise" />
            </button>
            <button class="btn btn-sm btn-primary join-item" title="Save Playlist" @click="savePlaylist(listDate)">
                <i class="bi-download" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                title="Delete Playlist"
                data-bs-toggle="modal"
                data-bs-target="#deleteModal"
            >
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
                        <div class="browser-col">
                            <div class="nav nav-tabs" id="v-pills-tab" role="tablist" aria-orientation="vertical">
                                <button
                                    class="nav-link active"
                                    id="v-pills-gui-tab"
                                    data-bs-toggle="pill"
                                    data-bs-target="#v-pills-gui"
                                    type="button"
                                    role="tab"
                                    aria-controls="v-pills-gui"
                                    aria-selected="true"
                                    @click="advancedGenerator = false"
                                >
                                    Simple
                                </button>
                                <button
                                    class="nav-link"
                                    id="v-pills-playout-tab"
                                    data-bs-toggle="pill"
                                    data-bs-target="#v-pills-playout"
                                    type="button"
                                    role="tab"
                                    aria-controls="v-pills-playout"
                                    aria-selected="false"
                                    @click=";(advancedGenerator = true), resetCheckboxes()"
                                >
                                    Advanced
                                </button>
                            </div>
                            <div class="tab-content h-100" id="v-pills-tabContent">
                                <div
                                    class="tab-pane h-100 show active"
                                    id="v-pills-gui"
                                    role="tabpanel"
                                    aria-labelledby="v-pills-gui-tab"
                                >
                                    <div class="h-100">
                                        <nav aria-label="breadcrumb">
                                            <ol class="breadcrumb border-0">
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
                                                        href="#"
                                                    >
                                                        {{ crumb.text }}
                                                    </a>
                                                    <span v-else>{{ crumb.text }}</span>
                                                </li>
                                            </ol>
                                        </nav>
                                        <ul class="list-group media-browser-scroll browser-div">
                                            <li
                                                class="list-group-item browser-item"
                                                v-for="folder in mediaStore.folderList.folders"
                                                :key="folder.uid"
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
                                                                        `/${mediaStore.folderList.source}/${folder.name}`.replace(
                                                                            /\/[/]+/g,
                                                                            '/'
                                                                        ),
                                                                        true
                                                                    ),
                                                                ]
                                                            "
                                                        >
                                                            {{ folder.name }}
                                                        </a>
                                                    </div>
                                                    <div
                                                        v-if="!generateFromAll"
                                                        class="col-1 text-center playlist-input"
                                                    >
                                                        <input
                                                            class="form-check-input folder-check"
                                                            type="checkbox"
                                                            @change="
                                                                setSelectedFolder(
                                                                    $event,
                                                                    `/${mediaStore.folderList.source}/${folder.name}`.replace(
                                                                        /\/[/]+/g,
                                                                        '/'
                                                                    )
                                                                )
                                                            "
                                                        />
                                                    </div>
                                                </div>
                                            </li>
                                        </ul>
                                    </div>
                                </div>
                                <div
                                    class="tab-pane"
                                    id="v-pills-playout"
                                    role="tabpanel"
                                    aria-labelledby="v-pills-playout-tab"
                                >
                                    <div>
                                        <div class="row">
                                            <div class="col col-10">
                                                <nav aria-label="breadcrumb">
                                                    <ol class="breadcrumb border-0">
                                                        <li
                                                            class="breadcrumb-item"
                                                            v-for="(crumb, index) in mediaStore.folderCrumbs"
                                                            :key="index"
                                                            :active="index === mediaStore.folderCrumbs.length - 1"
                                                            @click.prevent="mediaStore.getTree(crumb.path, true)"
                                                        >
                                                            <a
                                                                v-if="
                                                                    mediaStore.folderCrumbs.length > 1 &&
                                                                    mediaStore.folderCrumbs.length - 1 > index
                                                                "
                                                                href="#"
                                                            >
                                                                {{ crumb.text }}
                                                            </a>
                                                            <span v-else>{{ crumb.text }}</span>
                                                        </li>
                                                    </ol>
                                                </nav>
                                            </div>
                                            <div class="col d-flex justify-content-end">
                                                <button
                                                    type="button"
                                                    class="btn btn-primary p-2 py-0 m-1"
                                                    @click="addTemplate()"
                                                >
                                                    <i class="bi bi-folder-plus"></i>
                                                </button>
                                            </div>
                                        </div>
                                        <div class="row">
                                            <div class="col col-5 browser-col">
                                                <Sortable
                                                    :list="mediaStore.folderList.folders"
                                                    :options="templateBrowserSortOptions"
                                                    item-key="uid"
                                                    class="list-group media-browser-scroll browser-div"
                                                    tag="ul"
                                                >
                                                    <template #item="{ element, index }">
                                                        <li
                                                            :id="`adv_folder_${index}`"
                                                            class="draggable list-group-item browser-item"
                                                            :key="element.uid"
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
                                                                                    `/${mediaStore.folderList.source}/${element.name}`.replace(
                                                                                        /\/[/]+/g,
                                                                                        '/'
                                                                                    ),
                                                                                    true
                                                                                ),
                                                                            ]
                                                                        "
                                                                    >
                                                                        {{ element.name }}
                                                                    </a>
                                                                </div>
                                                            </div>
                                                        </li>
                                                    </template>
                                                </Sortable>
                                            </div>
                                            <div class="col template-col">
                                                <ul class="list-group media-browser-scroll">
                                                    <li
                                                        v-for="item in template.sources"
                                                        :key="item.start"
                                                        class="list-group-item"
                                                    >
                                                        <div class="input-group mb-3">
                                                            <span class="input-group-text">Start</span>
                                                            <input
                                                                type="test"
                                                                class="form-control"
                                                                aria-label="Start"
                                                                v-model="item.start"
                                                            />
                                                            <span class="input-group-text">Duration</span>
                                                            <input
                                                                type="test"
                                                                class="form-control"
                                                                aria-label="Duration"
                                                                v-model="item.duration"
                                                            />
                                                            <input
                                                                type="checkbox"
                                                                class="btn-check"
                                                                :id="`shuffle-${item.start}`"
                                                                autocomplete="off"
                                                                v-model="item.shuffle"
                                                            />
                                                            <label
                                                                class="btn btn-outline-primary"
                                                                :for="`shuffle-${item.start}`"
                                                            >
                                                                Shuffle
                                                            </label>
                                                        </div>

                                                        <Sortable
                                                            :list="item.paths"
                                                            item-key="index"
                                                            class="list-group w-100 border"
                                                            :style="`height: ${
                                                                item.paths ? item.paths.length * 23 + 31 : 300
                                                            }px`"
                                                            tag="ul"
                                                            :options="templateTargetSortOptions"
                                                            @add="addFolderToTemplate($event, item)"
                                                        >
                                                            <template #item="{ element, index }">
                                                                <li
                                                                    :id="`path_${index}`"
                                                                    class="draggable grabbing list-group-item py-0"
                                                                    :key="index"
                                                                >
                                                                    {{ element.split(/[\\/]+/).pop() }}
                                                                </li>
                                                            </template>
                                                        </Sortable>

                                                        <div class="col d-flex justify-content-end">
                                                            <button
                                                                type="button"
                                                                class="btn btn-primary p-2 py-0 m-1"
                                                                @click="removeTemplate(item)"
                                                            >
                                                                <i class="bi-trash" />
                                                            </button>
                                                        </div>
                                                    </li>
                                                </ul>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                    <div class="modal-footer">
                        <div v-if="!advancedGenerator" class="form-check select-all-div">
                            <input
                                id="checkAll"
                                class="form-check-input"
                                type="checkbox"
                                v-model="generateFromAll"
                                @change="resetCheckboxes()"
                            />
                            <label class="form-check-label" for="checkAll">All</label>
                        </div>
                        <button
                            type="button"
                            class="btn btn-primary"
                            data-bs-dismiss="modal"
                            @click="resetCheckboxes(), resetTemplate()"
                        >
                            Cancel
                        </button>
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

const advancedGenerator = ref(false)
const fileImport = ref()
const playlistIsLoading = ref(false)
const todayDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const listDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const targetDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const editId = ref(-1)
const textFile = ref()

const showPreviewModal = ref(false)
const showSourceModal = ref(false)

const previewName = ref('')
const previewUrl = ref('')
const previewOpt = ref()
const isVideo = ref(false)
const selectedFolders = ref([] as string[])
const generateFromAll = ref(false)
const browserSortOptions = {
    group: { name: 'playlist', pull: 'clone', put: false },
    sort: false,
}
const playlistSortOptions = {
    group: 'playlist',
    animation: 100,
    handle: '.grabbing',
}
const templateBrowserSortOptions = {
    group: { name: 'folder', pull: 'clone', put: false },
    sort: false,
}
const templateTargetSortOptions = {
    group: 'folder',
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

const template = ref({
    sources: [],
} as Template)

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

    console.log('scroll')
    console.log('child', child)
    console.log('parent', parent)

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

function addFolderToTemplate(event: any, item: TemplateItem) {
    const o = event.oldIndex
    const n = event.newIndex

    event.item.remove()

    const storagePath = configStore.configPlayout.storage.path
    const navPath = mediaStore.folderCrumbs[mediaStore.folderCrumbs.length - 1].path
    const sourcePath = `${storagePath}/${navPath}/${mediaStore.folderList.folders[o].name}`.replace(/\/[/]+/g, '/')

    if (!item.paths.includes(sourcePath)) {
        item.paths.splice(n, 0, sourcePath)
    }
}

function removeTemplate(item: TemplateItem) {
    const index = template.value.sources.indexOf(item)

    template.value.sources.splice(index, 1)
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
            indexStore.msgAlert('alert-success', 'Import success!', 2)
            playlistStore.getPlaylist(listDate.value)
        })
        .catch((e: string) => {
            indexStore.msgAlert('alert-error', e, 4)
        })

    playlistIsLoading.value = false
    textFile.value = null
    fileImport.value.value = null
}

async function generatePlaylist() {
    playlistIsLoading.value = true
    let body = null as BodyObject | null

    if (selectedFolders.value.length > 0 && !generateFromAll.value) {
        body = { paths: selectedFolders.value }
    }

    if (advancedGenerator.value) {
        if (body) {
            body.template = template.value
        } else {
            body = { template: template.value }
        }
    }

    await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/generate/${listDate.value}`, {
        method: 'POST',
        headers: { ...contentType, ...authStore.authHeader },
        body,
    })
        .then((response: any) => {
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                response.program,
                false
            )
            indexStore.msgAlert('alert-success', 'Generate Playlist done...', 2)
        })
        .catch((e: any) => {
            indexStore.msgAlert('alert-error', e.data ? e.data : e, 4)
        })

    // reset selections
    resetCheckboxes()
    resetTemplate()

    playlistIsLoading.value = false
}

async function savePlaylist(saveDate: string) {
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
            date: saveDate,
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

async function deletePlaylist(playlistDate: string) {
    await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/${playlistDate}`, {
        method: 'DELETE',
        headers: { ...contentType, ...authStore.authHeader },
    }).then(() => {
        playlistStore.playlist = []

        indexStore.msgAlert('alert-warning', 'Playlist deleted...', 2)
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

function resetCheckboxes() {
    selectedFolders.value = []
    const checkboxes = document.getElementsByClassName('folder-check')

    if (checkboxes) {
        for (const box of checkboxes) {
            // @ts-ignore
            box.checked = false
        }
    }
}

function resetTemplate() {
    template.value.sources = []
}

function addTemplate() {
    const last = template.value.sources[template.value.sources.length - 1]
    // @ts-ignore
    let start = $dayjs('00:00:00', 'HH:mm:ss')

    if (last) {
        // @ts-ignore
        const t = $dayjs(last.duration, 'HH:mm:ss')
        // @ts-ignore
        start = $dayjs(last.start, 'HH:mm:ss').add(t.hour(), 'hour').add(t.minute(), 'minute').add(t.second(), 'second')
    }

    template.value.sources.push({
        start: start.format('HH:mm:ss'),
        duration: '02:00:00',
        shuffle: false,
        paths: [],
    })
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
