<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

import { VueDatePicker } from '@vuepic/vue-datepicker'
import { de, enUS, ptBR, ru } from 'date-fns/locale'
import '@vuepic/vue-datepicker/dist/main.css'

// @ts-ignore
import { Splitpanes, Pane } from 'splitpanes'

import { computed, ref, onBeforeMount, onBeforeUnmount, watch } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { useI18n } from 'vue-i18n'
import { useWindowSize } from '@vueuse/core'
import { storeToRefs } from 'pinia'
import { useHead } from '@unhead/vue'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import PlayerControl from '@/components/player/PlayerControl.vue'
import MediaBrowser from '@/components/MediaBrowser.vue'
import PlaylistTable from '@/components/player/PlaylistTable.vue'
import GenericModal from '@/components/utils/GenericModal.vue'
import FileBrowserModal from '@/components/utils/FileBrowserModal.vue'
import PlaylistGenerator from '@/components/player/PlaylistGenerator.vue'
import VideoPlayer from '@/components/utils/VideoPlayer.vue'
import TimePicker from '@/components/utils/TimePicker.vue'
import SvgIcon from '@/components/utils/SvgIcon.vue'

import { authFetch } from '@/composables/authFetch'
import { browseChannelPaths } from '@/composables/fileBrowser'
import { stringFormatter, playlistOperations } from '@/composables/helper'
import { createFilePreviewUrl, storagePreviewPath } from '@/composables/fileAccess'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'
import { usePlaylist } from '@/stores/playlist'

const { locale, t } = useI18n()
const { width } = useWindowSize({ initialWidth: 800 })
const { mediaType } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

const { listDate, firstLoad } = storeToRefs(usePlaylist())

const beforeDayStart = ref(false)
const targetDate = ref(dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const playlistTable = ref()
const editId = ref(-1)
const textFile = ref()
const lang = ref()

const showPreviewModal = ref(false)
const showSourceModal = ref(false)
const showImportModal = ref(false)
const showCopyModal = ref(false)
const showDeleteModal = ref(false)
const showPlaylistGenerator = ref(false)

const previewName = ref('')
const previewUrl = ref('')
const previewSource = ref<{ src: string; type: string }>()
const isVideo = ref(false)
const splitCount = ref(0)
const splitTimes = ref<SplitTime[]>([])

const newSource = ref({
    begin: 0,
    title: null,
    in: 0,
    out: 0,
    duration: 0,
    ad: false,
    source: '',
    audio: '',
    uid: '',
} as PlaylistItem)

const sourceBrowserField = ref<'source' | 'audio'>()
const inspectingSource = ref(false)
let sourceInspection: AbortController | undefined
const sourceBrowserPath = computed(() =>
    sourceBrowserField.value ? (newSource.value[sourceBrowserField.value] ?? '') : '',
)

function loadSourcePaths(path: string, initial: boolean, signal: AbortSignal) {
    const channel = configStore.channels[configStore.i]?.id
    if (!channel) throw new Error(t('error.notFound'))
    // Remote sources remain editable as URLs; local browsing starts at storage.
    const source = initial && /^[a-z][a-z0-9+.-]*:\/\//i.test(path) ? '' : path
    return browseChannelPaths(channel, source, initial, signal)
}

async function selectSourcePath(path: string) {
    const field = sourceBrowserField.value
    if (!field) return
    if (field === 'audio') {
        newSource.value.audio = path
        return
    }
    sourceInspection?.abort()
    const controller = new AbortController()
    sourceInspection = controller
    inspectingSource.value = true
    const channel = configStore.channels[configStore.i]?.id
    try {
        const info = await authFetch<{ duration: number }>(`/api/file/${channel}/media-info`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ source: path }),
            signal: controller.signal,
        })
        if (controller.signal.aborted) return
        newSource.value.source = path
        newSource.value.duration = info.duration
        newSource.value.out = info.duration
        newSource.value.in = 0
        splitCount.value = 0
        splitTimes.value = []
    } catch (reason) {
        if (!controller.signal.aborted)
            indexStore.msgAlert('error', reason instanceof Error ? reason.message : String(reason), 4)
    } finally {
        if (sourceInspection === controller) inspectingSource.value = false
    }
}

function cancelSourceInspection() {
    sourceInspection?.abort()
    inspectingSource.value = false
}
onBeforeUnmount(cancelSourceInspection)

watch(
    () => [configStore.channels[configStore.i]?.id, showSourceModal.value],
    () => {
        sourceBrowserField.value = undefined
        cancelSourceInspection()
    },
)

switch (locale.value) {
    case 'de':
        lang.value = de
        break
    case 'pt-br':
        lang.value = ptBR
        break
    case 'ru':
        lang.value = ru
        break
    default:
        lang.value = enUS
}

useHead({
    title: computed(() => t('button.player')),
    bodyAttrs: {
        class: computed(() => {
            if (showPlaylistGenerator.value) return 'overflow-hidden'

            return ''
        }),
    },
})

onBeforeMount(() => {
    const currentTime = dayjs().tz(configStore.timezone)

    if (
        firstLoad.value &&
        currentTime.format('YYYY-MM-DD') === playlistStore.listDate &&
        currentTime.format('HH:mm:ss') > '00:00:00' &&
        currentTime.format('HH:mm:ss') < configStore.playout.playlist.day_start
    ) {
        listDate.value = dayjs(playlistStore.listDate).subtract(1, 'day').format('YYYY-MM-DD')
        beforeDayStart.value = true
    }

    if (configStore.onetimeInfo && configStore.playout.playlist.infinit) {
        indexStore.msgAlert('warning', t('player.infinitInfo'), 7)
        configStore.onetimeInfo = false
    }
})

function closeGenerator() {
    showPlaylistGenerator.value = false
}

function closePlayer() {
    showPreviewModal.value = false
    isVideo.value = false
}

function previewBrowserFile(name: string) {
    return setPreviewData(
        storagePreviewPath(name, configStore.channels[configStore.i]?.storage ?? '', mediaStore.folderTree.source),
    )
}

async function setPreviewData(path: string) {
    const fullPath = storagePreviewPath(path, configStore.channels[configStore.i]?.storage ?? '')

    previewName.value = fullPath.split('/').slice(-1)[0] ?? ''
    showPreviewModal.value = true

    if (path.match(/^http/)) {
        previewUrl.value = path
    } else {
        try {
            previewUrl.value = await createFilePreviewUrl(configStore.channels[configStore.i]?.id, fullPath)
        } catch (error) {
            indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 5)
            return
        }
    }

    const ext = previewName.value.split('.').slice(-1)[0]?.toLowerCase()

    const fileType =
        mediaType(previewName.value) === 'audio'
            ? `audio/${ext}`
            : mediaType(previewName.value) === 'live'
              ? 'application/x-mpegURL'
              : `video/${ext}`

    if (configStore.playout.storage.extensions.includes(`${ext}`)) {
        isVideo.value = true
        previewSource.value = { src: previewUrl.value, type: fileType }
    } else {
        isVideo.value = false
    }
}

function splitClip() {
    splitTimes.value = []
    for (let i = 0; i < splitCount.value; i++) {
        splitTimes.value.push({ id: i, val: (newSource.value.out / (splitCount.value + 1)) * (i + 1) })
    }
}

function splitSource(pos: number) {
    for (let i = 0; i < splitTimes.value.length; i++) {
        const source = cloneDeep(newSource.value)
        source.out = splitTimes.value[i]?.val ?? 0

        if (i > 0) {
            source.in = splitTimes.value[i - 1]?.val ?? 0
        }

        if (pos === -1) {
            playlistStore.playlist.push(source)
        } else if (i === 0) {
            playlistStore.playlist[pos] = source
        } else {
            playlistStore.playlist.splice(pos + i, 0, source)
        }
    }

    const source = cloneDeep(newSource.value)
    source.in = splitTimes.value[splitCount.value - 1]?.val ?? 0

    if (pos === -1) {
        playlistStore.playlist.push(source)
    } else {
        playlistStore.playlist.splice(pos + splitCount.value, 0, source)
    }
}

function processSource(process: boolean) {
    if (process && inspectingSource.value) return
    cancelSourceInspection()
    showSourceModal.value = false

    if (process) {
        if (splitCount.value > 0) {
            splitSource(editId.value)
        } else if (editId.value === -1) {
            playlistStore.playlist.push(newSource.value)
        } else {
            playlistStore.playlist[editId.value] = newSource.value
        }

        processPlaylist(listDate.value, playlistStore.playlist, false)
    }

    editId.value = -1
    newSource.value = {
        begin: 0,
        title: '',
        in: 0,
        out: 0,
        duration: 0,
        ad: false,
        source: '',
        audio: '',
        uid: genUID(),
    }

    splitCount.value = 0
    splitTimes.value = []
}

function editPlaylistItem(i: number) {
    editId.value = i
    showSourceModal.value = true

    newSource.value = {
        begin: playlistStore.playlist[i]?.begin ?? 0,
        title: playlistStore.playlist[i]?.title,
        in: playlistStore.playlist[i]?.in ?? 0,
        out: playlistStore.playlist[i]?.out ?? 0,
        duration: playlistStore.playlist[i]?.duration ?? 0,
        ad: playlistStore.playlist[i]?.ad ?? false,
        source: playlistStore.playlist[i]?.source ?? '',
        audio: playlistStore.playlist[i]?.audio ?? '',
        uid: playlistStore.playlist[i]?.uid ?? '',
    }
}

function loopClips() {
    const tempList = []
    let length = 0

    while (length < configStore.playlistLength && playlistStore.playlist.length > 0) {
        for (const item of playlistStore.playlist) {
            if (length < configStore.playlistLength) {
                item.uid = genUID()
                tempList.push(cloneDeep(item))
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
        await authFetch<any>(
            `/api/file/${configStore.channels[configStore.i]?.id}/import?file=${textFile.value[0].name}&date=${
                listDate.value
            }`,
            {
                method: 'PUT',
                headers: authStore.authHeader,
                body: formData,
            },
        )
            .then(async (response) => {
                indexStore.msgAlert('success', response, 2)
                await playlistStore.getPlaylist(listDate.value)
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

        const saveList = processPlaylist(listDate.value, cloneDeep(playlistStore.playlist), true)

        await authFetch<any>(`/api/playlist/${configStore.channels[configStore.i]?.id}`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                channel: configStore.channels[configStore.i]?.name,
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
        await authFetch(`/api/playlist/${configStore.channels[configStore.i]?.id}/${listDate.value}`, {
            method: 'DELETE',
            headers: { ...configStore.contentType, ...authStore.authHeader },
        }).then(() => {
            playlistStore.playlist = []
            indexStore.msgAlert('warning', t('player.deleteSuccess'), 2)
        })
    }
}
</script>
<template>
    <div class="h-full">
        <PlayerControl />
        <div class="flex justify-end p-1">
            <div class="h-8 flex">
                <div class="text-warning flex-none flex justify-end p-2">
                    <div
                        v-if="firstLoad && beforeDayStart"
                        class="tooltip tooltip-right tooltip-warning"
                        :data-tip="t('player.dateYesterday')"
                    >
                        <SvgIcon name="warning" />
                    </div>
                </div>
                <VueDatePicker
                    v-if="!configStore.playout.playlist.infinit && configStore.playout.processing.mode !== 'folder'"
                    v-model="listDate"
                    :hide-navigation="['time']"
                    :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                    :formats="{ day: 'dd', input: 'EEEE dd. LLL yyyy' }"
                    :time-config="{ enableTimePicker: false }"
                    model-type="yyyy-MM-dd"
                    auto-apply
                    class="rounded border border-base-content/30"
                    :locale="lang"
                    :dark="indexStore.darkMode"
                    :ui="{ input: 'input input-sm text-right pe-3!' }"
                    required
                    @update:model-value=";((beforeDayStart = false), (firstLoad = false))"
                />
            </div>
        </div>
        <div class="p-1 min-h-65 h-[calc(100vh-800px)] xl:h-[calc(100vh-480px)]">
            <Splitpanes
                v-if="configStore.playout.processing.mode === 'playlist'"
                class="border border-base-content/30 rounded-sm shadow"
            >
                <Pane
                    v-if="width > 739"
                    class="relative h-full bg-base-300! rounded-s"
                    min-size="0"
                    max-size="80"
                    size="20"
                >
                    <MediaBrowser :preview="previewBrowserFile" />
                </Pane>
                <pane>
                    <PlaylistTable ref="playlistTable" :edit-item="editPlaylistItem" :preview="setPreviewData" />
                </pane>
            </Splitpanes>
            <div v-else class="h-full border border-b-2 border-base-content/30 rounded-sm shadow">
                <MediaBrowser :preview="previewBrowserFile" />
            </div>
        </div>

        <div v-if="configStore.playout.processing.mode === 'playlist'" class="h-16 join flex justify-end p-3">
            <button class="btn btn-sm btn-primary join-item" :title="t('player.copy')" @click="showCopyModal = true">
                <i class="bi-files" />
            </button>
            <button
                v-if="!configStore.playout.playlist.infinit"
                class="btn btn-sm btn-primary join-item"
                :title="t('player.loop')"
                @click="loopClips()"
            >
                <i class="bi-view-stacked" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.remote')"
                @click="showSourceModal = true"
            >
                <i class="bi-file-earmark-plus" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.import')"
                @click="showImportModal = true"
            >
                <i class="bi-file-text" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.generate')"
                @click="(mediaStore.getTree('', true), (showPlaylistGenerator = true))"
            >
                <i class="bi-sort-down-alt" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.reset')"
                @click=";((playlistStore.playlist.length = 0), playlistTable.getPlaylist())"
            >
                <i class="bi-arrow-counterclockwise" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.save')"
                @click=";((targetDate = listDate), savePlaylist(true))"
            >
                <i class="bi-download" />
            </button>
            <button
                class="btn btn-sm btn-primary join-item"
                :title="t('player.deletePlaylist')"
                @click="showDeleteModal = true"
            >
                <i class="bi-trash" />
            </button>
        </div>

        <GenericModal
            :show="showPreviewModal"
            :title="`${t('media.preview')}: ${previewName}`"
            :hide-buttons="true"
            :modal-action="closePlayer"
        >
            <div class="w-5xl max-w-full aspect-video">
                <VideoPlayer v-if="isVideo && previewSource" :src="previewSource.src" :type="previewSource.type" />
                <img v-else :src="previewUrl" class="img-fluid" :alt="previewName" />
            </div>
        </GenericModal>

        <GenericModal :show="showCopyModal" :title="t('player.copyTo')" :modal-action="savePlaylist">
            <VueDatePicker
                v-model="targetDate"
                :clearable="false"
                :hide-navigation="['time']"
                :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                :formats="{ day: 'dd', input: 'EEEE dd. LLL yyyy' }"
                :time-config="{ enableTimePicker: false }"
                model-type="yyyy-MM-dd"
                auto-apply
                class="rounded border border-base-content/30"
                :locale="lang"
                :dark="indexStore.darkMode"
                :ui="{ input: 'input input-sm w-full text-right pe-3' }"
                required
            />
        </GenericModal>

        <GenericModal :show="showSourceModal" :title="t('player.addEdit')" :modal-action="processSource">
            <div class="lg:w-96">
                <div v-if="inspectingSource" role="status" class="flex justify-center py-2">
                    <span class="loading loading-spinner loading-sm" :aria-label="t('fileBrowser.loading')" />
                </div>
                <fieldset :disabled="inspectingSource">
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.title') }}</legend>
                        <input v-model="newSource.title" type="text" name="source" class="input input-sm w-full" />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.duration') }}</legend>
                        <TimePicker v-model="newSource.duration" />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.in') }}</legend>
                        <TimePicker v-model="newSource.in" />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.out') }}</legend>
                        <TimePicker v-model="newSource.out" />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.file') }}</legend>
                        <div class="join w-full">
                            <input
                                v-model="newSource.source"
                                type="text"
                                name="source"
                                class="input input-sm join-item w-full min-w-0"
                            />
                            <button
                                type="button"
                                class="btn btn-sm join-item"
                                :title="t('fileBrowser.title')"
                                :aria-label="t('fileBrowser.title')"
                                @click="sourceBrowserField = 'source'"
                            >
                                <i class="bi-folder2-open" />
                            </button>
                        </div>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.audio') }}</legend>
                        <div class="join w-full">
                            <input
                                v-model="newSource.audio"
                                type="text"
                                name="audio"
                                class="input input-sm join-item w-full min-w-0"
                                :placeholder="t('player.audioSourcePlaceholder')"
                            />
                            <button
                                type="button"
                                class="btn btn-sm join-item"
                                :title="t('fileBrowser.title')"
                                :aria-label="t('fileBrowser.title')"
                                @click="sourceBrowserField = 'audio'"
                            >
                                <i class="bi-folder2-open" />
                            </button>
                        </div>
                    </fieldset>
                    <fieldset class="fieldset mt-2 rounded-box w-full">
                        <label class="fieldset-label text-base-content">
                            <input v-model="newSource.ad" type="checkbox" class="checkbox" />
                            {{ t('player.ad') }}
                        </label>
                    </fieldset>

                    <hr class="h-px my-2 bg-base-content/20 border-0" />

                    <h4 class="font-bold">{{ t('player.splitVideo') }}</h4>

                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('player.cuts') }}</legend>
                        <input
                            v-model="splitCount"
                            type="number"
                            min="0"
                            step="1"
                            class="input input-sm w-full"
                            @change="splitClip"
                        />
                    </fieldset>

                    <div v-for="time in splitTimes" :key="time.id" class="form-control mt-2">
                        <TimePicker v-model="time.val" />
                    </div>
                </fieldset>
            </div>
        </GenericModal>

        <FileBrowserModal
            :show="Boolean(sourceBrowserField)"
            :initial-path="sourceBrowserPath"
            path-format="absolute"
            :extensions="configStore.playout.storage.extensions"
            :load="loadSourcePaths"
            @select="selectSourcePath"
            @close="sourceBrowserField = undefined"
        />

        <GenericModal :show="showImportModal" :title="t('player.import')" :modal-action="importPlaylist">
            <input type="file" class="file-input file-input-sm w-full" multiple @change="onFileChange" />
        </GenericModal>

        <GenericModal :show="showDeleteModal" title="Delete Program" :modal-action="deletePlaylist">
            <span>
                {{ t('player.deleteFrom') }} <strong>{{ listDate }}</strong>
            </span>
        </GenericModal>

        <PlaylistGenerator v-if="showPlaylistGenerator" :close="closeGenerator" />
    </div>
</template>
