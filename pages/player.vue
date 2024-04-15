<template>
    <div class="h-full">
        <Control />
        <div class="flex justify-end p-1">
            <div>
                <VueDatePicker
                    v-model="listDate"
                    :clearable="false"
                    :hide-navigation="['time']"
                    :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                    :format="calendarFormat"
                    model-type="yyyy-MM-dd"
                    auto-apply
                    :locale="locale"
                    :dark="colorMode.value === 'dark'"
                    input-class-name="input input-sm !input-bordered !w-[250px] text-right !pe-3"
                    required
                />
            </div>
        </div>
        <div class="p-1 min-h-[260px] h-[calc(100vh-800px)] xl:h-[calc(100vh-480px)]">
            <splitpanes class="border border-my-gray rounded shadow">
                <pane
                    v-if="width > 768"
                    class="relative h-full !bg-base-300 rounded-s"
                    min-size="0"
                    max-size="80"
                    size="20"
                >
                    <div
                        v-if="mediaStore.isLoading"
                        class="h-full w-full absolute z-10 flex justify-center bg-base-100/70"
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

                    <div class="w-full h-[calc(100%-48px)] overflow-auto m-1">
                        <div class="flex px-1" v-for="folder in mediaStore.folderTree.folders" :key="folder.uid">
                            <button
                                class="truncate"
                                @click="mediaStore.getTree(`/${mediaStore.folderTree.source}/${folder.name}`)"
                            >
                                <i class="bi-folder-fill" />
                                {{ folder.name }}
                            </button>
                        </div>
                        <Sortable
                            :list="mediaStore.folderTree.files"
                            :options="browserSortOptions"
                            item-key="name"
                            tag="table"
                            class="w-full table table-fixed"
                        >
                            <template #item="{ element, index }">
                                <tr
                                    :id="`file-${index}`"
                                    class="w-full"
                                    :class="{ 'grabbing cursor-grab': width > 768 }"
                                    :key="element.name"
                                >
                                    <td class="ps-1 py-1 w-[20px]">
                                        <i v-if="mediaType(element.name) === 'audio'" class="bi-music-note-beamed" />
                                        <i v-else-if="mediaType(element.name) === 'video'" class="bi-film" />
                                        <i
                                            v-else-if="mediaType(element.name) === 'image'"
                                            class="bi-file-earmark-image"
                                        />
                                        <i v-else class="bi-file-binary" />
                                    </td>
                                    <td class="px-[1px] py-1 truncate">
                                        {{ element.name }}
                                    </td>
                                    <td class="px-1 py-1 w-[30px] text-center leading-3">
                                        <button @click=";(showPreviewModal = true), setPreviewData(element.name)">
                                            <i class="bi-play-fill" />
                                        </button>
                                    </td>
                                    <td class="px-0 py-1 w-[65px] text-nowrap">
                                        {{ secToHMS(element.duration) }}
                                    </td>
                                    <td class="py-1 hidden">00:00:00</td>
                                    <td class="py-1 hidden">{{ secToHMS(element.duration) }}</td>
                                    <td class="py-1 hidden">&nbsp;</td>
                                    <td class="py-1 hidden">&nbsp;</td>
                                    <td class="py-1 hidden">&nbsp;</td>
                                </tr>
                            </template>
                        </Sortable>
                    </div>
                </pane>
                <pane>
                    <div id="playlist-container" class="relative w-full h-full !bg-base-300 rounded-e overflow-auto">
                        <div
                            v-if="playlistStore.isLoading"
                            class="w-full h-full absolute z-10 flex justify-center bg-base-100/70"
                        >
                            <span class="loading loading-spinner loading-lg" />
                        </div>
                        <table class="table table-zebra table-fixed">
                            <thead class="top-0 sticky z-10">
                                <tr class="bg-base-100 rounded-tr-lg">
                                    <th class="w-[85px] p-0 text-left">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.start') }}
                                        </div>
                                    </th>
                                    <th class="w-auto p-0 text-left">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.file') }}
                                        </div>
                                    </th>
                                    <th class="w-[90px] p-0 text-center">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.play') }}
                                        </div>
                                    </th>
                                    <th class="w-[85px] p-0 text-center">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.duration') }}
                                        </div>
                                    </th>
                                    <th class="w-[85px] p-0 text-center hidden xl:table-cell">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.in') }}
                                        </div>
                                    </th>
                                    <th class="w-[85px] p-0 text-center hidden xl:table-cell">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.out') }}
                                        </div>
                                    </th>
                                    <th class="w-[85px] p-0 text-center hidden xl:table-cell justify-center">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.ad') }}
                                        </div>
                                    </th>
                                    <th class="w-[95px] p-0 text-center">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.edit') }}
                                        </div>
                                    </th>
                                    <th class="w-[85px] p-0 text-center hidden xl:table-cell justify-center">
                                        <div class="border-b border-my-gray px-4 py-3 -mb-[2px]">
                                            {{ $t('player.delete') }}
                                        </div>
                                    </th>
                                </tr>
                            </thead>
                            <Sortable
                                :list="playlistStore.playlist"
                                item-key="uid"
                                tag="tbody"
                                :options="playlistSortOptions"
                                @add="addClip"
                                @start="addBG"
                                @end="moveItemInArray"
                            >
                                <template #item="{ element, index }">
                                    <tr
                                        :id="`clip-${index}`"
                                        class="draggable border-t border-b border-base-content/20 duration-1000 transition-all"
                                        :class="{
                                            '!bg-lime-500/30':
                                                playlistStore.playoutIsRunning &&
                                                listDate === todayDate &&
                                                index === playlistStore.currentClipIndex,
                                        }"
                                        :key="element.uid"
                                    >
                                        <td class="ps-4 py-2 text-left">{{ secondsToTime(element.begin) }}</td>
                                        <td
                                            class="py-2 text-left truncate"
                                            :class="{ 'grabbing cursor-grab': width > 768 }"
                                        >
                                            {{ filename(element.source) }}
                                        </td>
                                        <td class="py-2 text-center hover:text-base-content/70">
                                            <button @click=";(showPreviewModal = true), setPreviewData(element.source)">
                                                <i class="bi-play-fill" />
                                            </button>
                                        </td>
                                        <td class="py-2 text-center">{{ secToHMS(element.duration) }}</td>
                                        <td class="py-2 text-center hidden xl:table-cell">
                                            {{ secToHMS(element.in) }}
                                        </td>
                                        <td class="py-2 text-center hidden xl:table-cell">
                                            {{ secToHMS(element.out) }}
                                        </td>
                                        <td class="py-2 text-center hidden xl:table-cell leading-3">
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
                                        </td>
                                        <td class="py-2 text-center hover:text-base-content/70">
                                            <button @click=";(showSourceModal = true), editPlaylistItem(index)">
                                                <i class="bi-pencil-square" />
                                            </button>
                                        </td>
                                        <td
                                            class="py-2 text-center hidden xl:table-cell justify-center hover:text-base-content/70"
                                        >
                                            <button @click="deletePlaylistItem(index)">
                                                <i class="bi-x-circle-fill" />
                                            </button>
                                        </td>
                                    </tr>
                                </template>
                            </Sortable>
                        </table>
                    </div>
                </pane>
            </splitpanes>
        </div>

        <div class="h-16 join flex justify-end p-3">
            <button class="btn btn-sm btn-primary join-item" :title="$t('player.copy')" @click="showCopyModal = true">
                <i class="bi-files" />
            </button>
            <button
                v-if="!configStore.configPlayout.playlist.loop"
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.loop')"
                @click="loopClips()"
            >
                <i class="bi-view-stacked" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.remote')"
                @click="showSourceModal = true"
            >
                <i class="bi-file-earmark-plus" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.import')"
                @click="showImportModal = true"
            >
                <i class="bi-file-text" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.generate')"
                @click="mediaStore.getTree('', true), (showPlaylistGenerator = true)"
            >
                <i class="bi-sort-down-alt" />
            </button>
            <button class="btn btn-sm btn-primary join-item" :title="$t('player.reset')" @click="getPlaylist()">
                <i class="bi-arrow-counterclockwise" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.save')"
                @click=";(targetDate = listDate), savePlaylist(true)"
            >
                <i class="bi-download" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.deletePlaylist')"
                @click="showDeleteModal = true"
            >
                <i class="bi-trash" />
            </button>
        </div>

        <Modal
            :show="showPreviewModal"
            :title="`Preview: ${previewName}`"
            :hide-buttons="true"
            :modal-action="closePlayer"
        >
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

        <PlaylistGenerator v-if="showPlaylistGenerator" :close="closeGenerator" />
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const colorMode = useColorMode()
const { locale } = useI18n()
const { $_, $dayjs } = useNuxtApp()
const { width } = useWindowSize({ initialWidth: 800 })
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
    handle: '.grabbing',
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

onMounted(() => {
    if (!mediaStore.folderTree.parent) {
        mediaStore.getTree('')
    }

    getPlaylist()
})

watch([configID], () => {
    mediaStore.getTree('')
})

watch([listDate], async () => {
    await getPlaylist()
})

function scrollTo(index: number) {
    const child = document.getElementById(`clip-${index}`)
    const parent = document.getElementById('playlist-container')

    if (child && parent) {
        const topPos = child.offsetTop
        parent.scrollTop = topPos - 50
    }
}

const calendarFormat = (date: Date) => {
    return $dayjs(date).locale(locale.value).format('dddd - LL')
}

async function getPlaylist() {
    playlistStore.isLoading = true
    await playlistStore.getPlaylist(listDate.value)
    playlistStore.isLoading = false

    if (listDate.value === todayDate.value) {
        scrollTo(playlistStore.currentClipIndex)
    } else {
        scrollTo(0)
    }
}

function closeGenerator() {
    showPlaylistGenerator.value = false
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

function addBG(obj: any) {
    if (obj.item) {
        obj.item.classList.add('!bg-fuchsia-900/30')
    } else {
        obj.classList.add('!bg-fuchsia-900/30')
    }
}

function removeBG(item: any) {
    setTimeout(() => {
        item.classList.remove('!bg-fuchsia-900/30')
    }, 100)
}

function addClip(event: any) {
    const o = event.oldIndex
    const n = event.newIndex
    const uid = genUID()

    event.item.remove()

    const storagePath = configStore.configPlayout.storage.path
    const sourcePath = `${storagePath}/${mediaStore.folderTree.source}/${mediaStore.folderTree.files[o].name}`.replace(
        /\/[/]+/g,
        '/'
    )

    playlistStore.playlist.splice(n, 0, {
        uid,
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

    nextTick(() => {
        const newNode = document.getElementById(`clip-${n}`)
        addBG(newNode)
        removeBG(newNode)
    })
}

function moveItemInArray(event: any) {
    playlistStore.playlist.splice(event.newIndex, 0, playlistStore.playlist.splice(event.oldIndex, 1)[0])

    playlistStore.playlist = processPlaylist(
        configStore.startInSec,
        configStore.playlistLength,
        playlistStore.playlist,
        false
    )

    removeBG(event.item)
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
                item.uid = genUID()
                tempList.push($_.cloneDeep(item))
                length += item.out - item.in
            } else {
                break
            }
        }
    }

    playlistStore.playlist = processPlaylist(configStore.startInSec, configStore.playlistLength, tempList, false)
}

function onFileChange(evt: any) {
    const files = evt.target.files || evt.dataTransfer.files

    if (!files.length) {
        return
    }

    textFile.value = files
}

async function importPlaylist(imp: boolean) {
    showImportModal.value = false

    if (imp) {
        if (!textFile.value || !textFile.value[0]) {
            return
        }

        const formData = new FormData()
        formData.append(textFile.value[0].name, textFile.value[0])

        playlistStore.isLoading = true
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
            .then((response) => {
                indexStore.msgAlert('success', response, 2)
                playlistStore.getPlaylist(listDate.value)
            })
            .catch((e: string) => {
                indexStore.msgAlert('error', e, 4)
            })
    }

    playlistStore.isLoading = false
    textFile.value = null
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
                indexStore.msgAlert('success', response, 2)
            })
            .catch((e: any) => {
                if (e.status === 409) {
                    indexStore.msgAlert('warning', e.data, 2)
                } else {
                    indexStore.msgAlert('error', e, 4)
                }
            })
    }
}

async function deletePlaylist(del: boolean) {
    showDeleteModal.value = false

    if (del) {
        await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/${listDate.value}`, {
            method: 'DELETE',
            headers: { ...contentType, ...authStore.authHeader },
        }).then(() => {
            playlistStore.playlist = []

            indexStore.msgAlert('warning', 'Playlist deleted...', 2)
        })
    }
}
</script>
<style>
/*
    format dragging element
*/
#playlist-container .sortable-ghost {
    background-color: #701a754b !important;
    min-height: 37px !important;
    height: 37px !important;
}

#playlist-container .sortable-ghost td {
    padding-left: 1rem;
    padding-right: 1rem;
    padding-top: 0.5rem;
    padding-bottom: 0.5rem;
}

#playlist-container .sortable-ghost td:nth-last-child(-n+5) {
    display: table-cell !important;
}
</style>
