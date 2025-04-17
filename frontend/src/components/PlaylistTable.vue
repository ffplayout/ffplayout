<template>
    <div class="relative w-full h-full !bg-base-300 rounded-e">
        <div v-if="playlistStore.isLoading" class="w-full h-full absolute z-2 flex justify-center bg-base-100/70">
            <span class="loading loading-spinner loading-lg" />
        </div>

        <div
            class="grid grid-cols-[85px_auto_85px_85px] 2md:grid-cols-[85px_auto_85px_85px_85px_85px_85px_85px_85px] bg-base-100 border-b border-base-content/30 rounded-tr-lg py-2 text-sm font-bold text-base-content/70"
        >
            <div v-if="!configStore.playout.playlist.infinit" class="px-3">
                {{ t('player.start') }}
            </div>
            <div>
                {{ t('player.file') }}
            </div>
            <div class="px-2 text-center">
                {{ t('player.play') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.duration') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.in') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.out') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.ad') }}
            </div>
            <div class="px-2 text-center">
                {{ t('player.edit') }}
            </div>
            <div class="px-2 hidden 2md:block text-center">
                {{ t('player.delete') }}
            </div>
        </div>

        <div id="playlist-container" ref="playlistContainer" class="h-[calc(100%-35px)]">
            <VirtualList
                id="sort-container"
                ref="sortContainer"
                v-model="playlistStore.playlist"
                :group="dragGroup"
                class="h-full"
                :handle="'.handle'"
                :data-key="'uid'"
                chosen-class="cursor-grabbing"
                ghost-class="sortable-ghost"
                placeholder-class="sortable-ghost"
                wrap-tag="ul"
                wrap-class="relative list-none text-sm"
                :tableMode="false"
                :animation="100"
                @drop="dropItem"
                @drag="addBG"
            >
                <template #header>
                    <div v-if="playlistStore.playlist.length === 0" class="is-empty relative">
                        <div class="absolute text-8xl text-base-content/40 text-center w-full mt-10">
                            <i class="bi-box-arrow-in-down" />
                        </div>
                    </div>
                </template>
                <template #item="{ record, index }">
                    <li
                        :id="record.uid"
                        :key="record.uid"
                        class="grid grid-cols-[85px_auto_85px_85px] 2md:grid-cols-[85px_auto_85px_85px_85px_85px_85px_85px_85px] odd:bg-base-200 border-b border-base-content/20 duration-500 transition-colors py-2"
                        :class="{
                            '!bg-lime-500/30':
                                playlistStore.playoutIsRunning && listDate === todayDate && index === currentIndex,
                            '!bg-amber-600/40': record.overtime,
                            'text-base-content/60': record.category === 'advertisement',
                        }"
                    >
                        <div v-if="!configStore.playout.playlist.infinit" class="px-3 text-left">
                            {{ secondsToTime(record.begin) }}
                        </div>
                        <div class="text-left truncate" :class="{ 'handle cursor-grab': width > 768 }">
                            {{ record.title || filename(record.source) }}
                        </div>
                        <div class="text-center hover:text-base-content/70">
                            <button class="cursor-pointer" @click="preview(record.source)">
                                <i class="bi-play-fill" />
                            </button>
                        </div>
                        <div class="text-center hidden 2md:block">{{ secToHMS(record.duration) }}</div>
                        <div class="text-center hidden 2md:block">
                            {{ secToHMS(record.in) }}
                        </div>
                        <div class="text-center hidden 2md:block">
                            {{ secToHMS(record.out) }}
                        </div>
                        <div class="text-center hidden 2md:block leading-3">
                            <input
                                class="checkbox checkbox-xs rounded"
                                type="checkbox"
                                :checked="record.category && record.category === 'advertisement' ? true : false"
                                @change="setCategory($event, record)"
                            />
                        </div>
                        <div class="text-center hover:text-base-content/70">
                            <button class="cursor-pointer" @click="editItem(index)">
                                <i class="bi-pencil-square" />
                            </button>
                        </div>
                        <div
                            class="text-center hidden 2md:block justify-center text-base-content/80 hover:text-base-content/60"
                        >
                            <button class="cursor-pointer" @click="deletePlaylistItem(index)">
                                <i class="bi-x-circle-fill" />
                            </button>
                        </div>
                    </li>
                </template>
            </VirtualList>
        </div>
    </div>
</template>
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

import VirtualList from 'vue-virtual-draglist'

import { ref, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useWindowSize, until } from '@vueuse/core'
import { storeToRefs } from 'pinia'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import { stringFormatter, playlistOperations } from '@/composables/helper'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'
import { usePlaylist } from '@/stores/playlist'

const { t } = useI18n()
const { width } = useWindowSize({ initialWidth: 800 })
const configStore = useConfig()
const mediaStore = useMedia()
const playlistStore = usePlaylist()
const { secToHMS, filename, secondsToTime } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const playlistContainer = ref()
const sortContainer = ref()
const todayDate = ref(dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const { i } = storeToRefs(useConfig())
const { currentIndex, listDate, playoutIsRunning, scrollToItem } = storeToRefs(usePlaylist())

const dragGroup = ref({ name: 'dragGroup', pull: true, put: true })

defineProps({
    editItem: {
        type: Function,
        default() {
            return ''
        },
    },
    preview: {
        type: Function,
        default() {
            return ''
        },
    },
})

onMounted(() => {
    setTimeout(() => {
        getPlaylist()
    }, 150)
})

watch([listDate, i], () => {
    setTimeout(() => {
        getPlaylist()
    }, 800)
})

watch([playoutIsRunning, scrollToItem], () => {
    if (playoutIsRunning.value || scrollToItem.value) {
        setTimeout(() => {
            scrollTo(currentIndex.value)

            scrollToItem.value = false
        }, 400)
    }
})

defineExpose({
    getPlaylist,
})

function scrollTo(index: number) {
    sortContainer.value.scrollToIndex(index)
}

async function getPlaylist() {
    playlistStore.isLoading = true
    await playlistStore.getPlaylist(listDate.value)
    playlistStore.isLoading = false

    if (listDate.value === todayDate.value) {
        await until(currentIndex).toMatch((v) => v > 0, { timeout: 1500 })
        scrollTo(currentIndex.value)
    } else {
        scrollTo(0)
    }
}

function setCategory(event: any, item: PlaylistItem) {
    if (event.target.checked) {
        item.category = 'advertisement'
    } else {
        item.category = ''
    }
}

function addBG(item: any) {
    if (item.event?.target) {
        item.event?.target.classList.add('!bg-fuchsia-900/30')
    } else {
        item.classList?.add('!bg-fuchsia-900/30')
    }
}

function removeBG(item: any) {
    setTimeout(() => {
        item.classList?.remove('!bg-fuchsia-900/30')
    }, 1000)
}

function dropItem(event: any) {
    const nIndex = event.newIndex

    if (event.event.from.id === 'mediaList') {
        const media = event.item
        const storagePath = configStore.channels[configStore.i].storage
        const sourcePath = `${storagePath}/${mediaStore.folderTree.source}/${media.name}`.replace(/\/[/]+/g, '/')
        const uid = genUID()

        const mediaObject = {
            uid,
            begin: 0,
            source: sourcePath,
            in: 0,
            out: media.duration || 10,
            duration: media.duration || 10,
        }

        playlistStore.playlist[nIndex] = mediaObject

        nextTick(() => {
            const newNode = document.getElementById(uid)
            addBG(newNode)
            removeBG(newNode)
        })
    }

    processPlaylist(listDate.value, playlistStore.playlist, false)

    nextTick(() => {
        removeBG(event.event.node)

        if (nIndex > playlistStore.playlist.length - 4) {
            sortContainer.value.scrollToBottom()
        }
    })
}

function deletePlaylistItem(index: number) {
    playlistStore.playlist.splice(index, 1)

    processPlaylist(listDate.value, playlistStore.playlist, false)
}
</script>
<style>
#sort-container .timeHidden {
    display: none !important;
}

/*
    format dragging elements
*/
.media-placeholder,
.sortable-ghost {
    background-color: #701a754b !important;
    min-height: 37px !important;
    height: 37px !important;
}

#playlist-container .media-placeholder div:nth-child(1)::after {
    content: '00:00:00';
    padding-left: 4px;
}

#playlist-container .media-placeholder div:nth-child(1) i {
    display: none;
}

#playlist-container .media-placeholder div:nth-child(4),
#playlist-container .media-placeholder div:nth-child(5),
#playlist-container .media-placeholder div:nth-child(6) {
    padding-left: 18px;
}

#playlist-container .media-placeholder {
    grid-template-columns: 85px auto 85px 85px;
}

@media (max-width: 875px) {
    #playlist-container .media-placeholder div:nth-child(4) {
        display: none;
    }
}

@media (min-width: 876px) {
    #playlist-container .media-placeholder {
        grid-template-columns: 85px auto 85px 85px 85px 85px 85px 85px 85px;
    }

    #playlist-container .media-placeholder div {
        display: block;
    }
}
</style>
