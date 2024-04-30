<template>
    <div class="h-full">
        <PlayerControl />
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
                    input-class-name="input input-sm !input-bordered !w-[300px] text-right !pe-3"
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
                    <MediaBrowser :preview="setPreviewData" />
                </pane>
                <pane>
                    <PlaylistTable ref="playlistTable" :edit-item="editPlaylistItem" :preview="setPreviewData" />
                </pane>
            </splitpanes>
        </div>

        <div class="h-16 join flex justify-end p-3">
            <button class="btn btn-sm btn-primary join-item" :title="$t('player.copy')" @click="showCopyModal = true">
                <i class="bi-files" />
            </button>
            <button
                v-if="!configStore.playout.playlist.loop"
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
            <button
                class="btn btn-sm btn-primary join-item"
                :title="$t('player.reset')"
                @click=";(playlistStore.playlist.length = 0), playlistTable.getPlaylist()"
            >
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

        <GenericModal
            :show="showPreviewModal"
            :title="`${$t('media.preview')}: ${previewName}`"
            :hide-buttons="true"
            :modal-action="closePlayer"
        >
            <div class="w-[1024px] max-w-full aspect-video">
                <VideoPlayer v-if="isVideo && previewOpt" reference="previewPlayer" :options="previewOpt" />
                <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
            </div>
        </GenericModal>

        <GenericModal :show="showCopyModal" :title="$t('player.copyTo')" :modal-action="savePlaylist">
            <VueDatePicker
                v-model="targetDate"
                :clearable="false"
                :hide-navigation="['time']"
                :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                :format="calendarFormat"
                model-type="yyyy-MM-dd"
                auto-apply
                :locale="locale"
                :dark="colorMode.value === 'dark'"
                input-class-name="input input-sm !input-bordered !w-[full text-right !pe-3"
                required
            />
        </GenericModal>

        <GenericModal :show="showSourceModal" :title="$t('player.addEdit')" :modal-action="processSource">
            <div>
                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.title') }}</span>
                    </div>
                    <input v-model.number="newSource.title" type="text" class="input input-sm input-bordered w-auto" />
                </label>
                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.timeIn') }}</span>
                    </div>
                    <input v-model.number="newSource.in" type="number" class="input input-sm input-bordered w-auto" />
                </label>

                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.timeOut') }}</span>
                    </div>
                    <input v-model.number="newSource.out" type="number" class="input input-sm input-bordered w-auto" />
                </label>

                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.timeDuration') }}</span>
                    </div>
                    <input
                        v-model.number="newSource.duration"
                        type="number"
                        class="input input-sm input-bordered w-auto"
                    />
                </label>

                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.file') }}</span>
                    </div>
                    <input v-model="newSource.source" type="text" class="input input-sm input-bordered w-auto" />
                </label>

                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.audio') }}</span>
                    </div>
                    <input v-model="newSource.audio" type="text" class="input input-sm input-bordered w-auto" />
                </label>

                <label class="form-control w-auto mt-auto">
                    <div class="label">
                        <span class="label-text">{{ $t('player.customFilter') }}</span>
                    </div>
                    <input v-model="newSource.custom_filter" type="text" class="input input-sm input-bordered w-auto" />
                </label>

                <div class="form-control">
                    <label class="cursor-pointer label">
                        <span class="label-text">{{ $t('player.ad') }}</span>
                        <input type="checkbox" class="checkbox checkbox-sm" @click="isAd" />
                    </label>
                </div>
            </div>
        </GenericModal>

        <GenericModal :show="showImportModal" :title="$t('player.import')" :modal-action="importPlaylist">
            <input
                type="file"
                class="file-input file-input-sm file-input-bordered w-full"
                multiple
                @change="onFileChange"
            />
        </GenericModal>

        <GenericModal :show="showDeleteModal" title="Delete Program" :modal-action="deletePlaylist">
            <span>
                {{ $t('player.deleteFrom') }} <strong>{{ listDate }}</strong>
            </span>
        </GenericModal>

        <PlaylistGenerator
            v-if="showPlaylistGenerator"
            :close="closeGenerator"
            :switch-class="playlistTable.classSwitcher"
        />
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const colorMode = useColorMode()
const { locale, t } = useI18n()
const { $_, $dayjs } = useNuxtApp()
const { width } = useWindowSize({ initialWidth: 800 })
const { mediaType } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

useHead({
    title: `${t('button.player')} | ffplayout`,
})

const { listDate } = storeToRefs(usePlaylist())

const targetDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const playlistTable = ref()
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

const newSource = ref({
    begin: 0,
    title: '',
    in: 0,
    out: 0,
    duration: 0,
    category: '',
    custom_filter: '',
    source: '',
    audio: '',
    uid: '',
} as PlaylistItem)

const calendarFormat = (date: Date) => {
    return $dayjs(date).locale(locale.value).format('dddd - LL')
}

function closeGenerator() {
    showPlaylistGenerator.value = false
}

function closePlayer() {
    showPreviewModal.value = false
    isVideo.value = false
}

function setPreviewData(path: string) {
    let fullPath = path
    const storagePath = configStore.playout.storage.path
    const lastIndex = storagePath.lastIndexOf('/')

    if (!path.includes('/')) {
        const parent = mediaStore.folderTree.parent ? mediaStore.folderTree.parent : ''
        fullPath = `/${parent}/${mediaStore.folderTree.source}/${path}`.replace(/\/[/]+/g, '/')
    } else if (lastIndex !== -1) {
        fullPath = path.replace(storagePath.substring(0, lastIndex), '')
    }

    previewName.value = fullPath.split('/').slice(-1)[0]
    showPreviewModal.value = true

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

function processSource(process: boolean) {
    showSourceModal.value = false

    if (process) {
        if (editId.value === -1) {
            playlistStore.playlist.push(newSource.value)
        } else {
            playlistStore.playlist[editId.value] = newSource.value
        }

        processPlaylist(listDate.value, playlistStore.playlist, false)
        playlistTable.value.classSwitcher()
    }

    editId.value = -1
    newSource.value = {
        begin: 0,
        title: '',
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
    showSourceModal.value = true

    newSource.value = {
        begin: playlistStore.playlist[i].begin,
        title: playlistStore.playlist[i].title,
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

    playlistStore.playlist = processPlaylist(listDate.value, tempList, false)
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
            .then(async (response) => {
                indexStore.msgAlert('success', String(response), 2)
                await playlistStore.getPlaylist(listDate.value)
                playlistTable.value.classSwitcher()
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

        const saveList = processPlaylist(listDate.value, $_.cloneDeep(playlistStore.playlist), true)

        await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                channel: configStore.configGui[configStore.configID].name,
                date: targetDate.value,
                program: saveList,
            }),
        })
            .then((response: any) => {
                playlistTable.value.classSwitcher()
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
            headers: { ...configStore.contentType, ...authStore.authHeader },
        }).then(() => {
            playlistStore.playlist = []
            playlistTable.value.classSwitcher()
            indexStore.msgAlert('warning', t('player.deleteSuccess'), 2)
        })
    }
}
</script>
