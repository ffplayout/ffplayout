<template>
    <div
        id="playlist-container"
        ref="playlistContainer"
        class="relative w-full h-full !bg-base-300 rounded-e overflow-auto"
    >
        <div v-if="playlistStore.isLoading" class="w-full h-full absolute z-2 flex justify-center bg-base-100/70">
            <span class="loading loading-spinner loading-lg" />
        </div>
        <table class="table table-zebra table-fixed">
            <thead class="top-0 sticky z-2">
                <tr class="bg-base-100 rounded-tr-lg">
                    <th v-if="!configStore.playout.playlist.infinit" class="w-[85px] p-0 text-left">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.start') }}
                        </div>
                    </th>
                    <th class="w-full p-0 text-left">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.file') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.play') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center hidden 2xs:table-cell">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.duration') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center hidden xl:table-cell">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.in') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center hidden xl:table-cell">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.out') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center hidden xl:table-cell justify-center">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.ad') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.edit') }}
                        </div>
                    </th>
                    <th class="w-[85px] p-0 text-center hidden 2xs:table-cell justify-center">
                        <div class="border-b border-my-gray px-4 py-3">
                            {{ t('player.delete') }}
                        </div>
                    </th>
                </tr>
            </thead>

            <Sortable
                id="sort-container"
                ref="sortContainer"
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
                        :key="element.uid"
                        class="draggable border-t border-b border-base-content/20 duration-1000 transition-all"
                        :class="{
                            '!bg-lime-500/30':
                                playlistStore.playoutIsRunning && listDate === todayDate && index === currentIndex,
                            '!bg-amber-600/40': element.overtime,
                            'text-base-content/60': element.category === 'advertisement',
                        }"
                    >
                        <td v-if="!configStore.playout.playlist.infinit" class="ps-4 py-2 text-left">
                            {{ secondsToTime(element.begin) }}
                        </td>
                        <td class="py-2 text-left truncate" :class="{ 'grabbing cursor-grab': width > 768 }">
                            {{ element.title || filename(element.source) }}
                        </td>
                        <td class="py-2 text-center hover:text-base-content/70">
                            <button @click="preview(element.source)">
                                <i class="bi-play-fill" />
                            </button>
                        </td>
                        <td class="py-2 text-center hidden 2xs:table-cell">{{ secToHMS(element.duration) }}</td>
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
                                :checked="element.category && element.category === 'advertisement' ? true : false"
                                @change="setCategory($event, element)"
                            />
                        </td>
                        <td class="py-2 text-center hover:text-base-content/70">
                            <button @click="editItem(index)">
                                <i class="bi-pencil-square" />
                            </button>
                        </td>
                        <td class="py-2 text-center hidden 2xs:table-cell justify-center hover:text-base-content/70">
                            <button @click="deletePlaylistItem(index)">
                                <i class="bi-x-circle-fill" />
                            </button>
                        </td>
                    </tr>
                </template>
            </Sortable>
        </table>
    </div>
</template>
<script setup lang="ts">
import { storeToRefs } from 'pinia'

const { t } = useI18n()
const { $dayjs } = useNuxtApp()
const { width } = useWindowSize({ initialWidth: 800 })

const configStore = useConfig()
const mediaStore = useMedia()
const playlistStore = usePlaylist()
const { secToHMS, filename, secondsToTime } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const playlistContainer = ref()
const sortContainer = ref()
const todayDate = ref($dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))
const { i } = storeToRefs(useConfig())
const { currentIndex, listDate, playoutIsRunning, scrollToItem } = storeToRefs(usePlaylist())

const playlistSortOptions = {
    group: 'playlist',
    animation: 100,
    handle: '.grabbing',
}

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
    classSwitcher,
    getPlaylist,
})

function scrollTo(index: number) {
    const child = document.getElementById(`clip-${index}`)
    const parent = document.getElementById('playlist-container')

    if (child && parent) {
        const topPos = child.offsetTop
        parent.scrollTop = topPos - 50
    }
}

function classSwitcher() {
    if (playlistStore.playlist.length === 0) {
        sortContainer.value?.sortable.el.classList.add('is-empty')
    } else {
        const lastItem = playlistStore.playlist[playlistStore.playlist.length - 1]

        if (
            configStore.playout.playlist.startInSec + configStore.playout.playlist.lengthInSec >
                lastItem.begin + lastItem.out - lastItem.in ||
            configStore.playout.playlist.infinit
        ) {
            sortContainer.value?.sortable.el.classList.add('add-space')
        } else {
            sortContainer.value?.sortable.el.classList.remove('add-space')
        }
        sortContainer.value?.sortable.el.classList.remove('is-empty')
    }
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

    classSwitcher()
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
        obj?.classList?.add('!bg-fuchsia-900/30')
    }
}

function removeBG(item: any) {
    setTimeout(() => {
        item?.classList?.remove('!bg-fuchsia-900/30')
    }, 100)
}

function addClip(event: any) {
    const o = event.oldIndex
    const n = event.newIndex
    const uid = genUID()

    event.item?.remove()

    const storagePath = configStore.channels[configStore.i].storage
    const sourcePath = `${storagePath}/${mediaStore.folderTree.source}/${mediaStore.folderTree.files[o].name}`.replace(
        /\/[/]+/g,
        '/'
    )

    playlistStore.playlist.splice(n, 0, {
        uid,
        begin: 0,
        source: sourcePath,
        in: 0,
        out: mediaStore.folderTree.files[o].duration || 10,
        duration: mediaStore.folderTree.files[o].duration || 10,
    })

    processPlaylist(listDate.value, playlistStore.playlist, false)
    classSwitcher()

    nextTick(() => {
        const newNode = document.getElementById(`clip-${n}`)
        addBG(newNode)
        removeBG(newNode)

        if (n > playlistStore.playlist.length - 3) {
            playlistContainer.value.scroll({ top: playlistContainer.value.scrollHeight, behavior: 'smooth' })
        }
    })
}

function moveItemInArray(event: any) {
    playlistStore.playlist.splice(event.newIndex, 0, playlistStore.playlist.splice(event.oldIndex, 1)[0])

    processPlaylist(listDate.value, playlistStore.playlist, false)

    removeBG(event.item)
}

function deletePlaylistItem(index: number) {
    playlistStore.playlist.splice(index, 1)

    processPlaylist(listDate.value, playlistStore.playlist, false)
    classSwitcher()
}
</script>
<style>
#sort-container.is-empty:not(:has(.sortable-ghost)):after {
    content: '\f1bc';
    font-family: 'bootstrap-icons';
    opacity: 0.3;
    font-size: 50px;
    width: 100%;
    height: 210px;
    display: flex;
    position: absolute;
    justify-content: center;
    align-items: center;
}

#sort-container.add-space:after {
    content: ' ';
    width: 100%;
    height: 37px;
    display: flex;
    position: absolute;
}

#sort-container .timeHidden {
    display: none !important;
}

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

#playlist-container .sortable-ghost td:nth-last-child(3),
#playlist-container .sortable-ghost td:nth-last-child(-n + 1) {
    display: table-cell !important;
}

@media (min-width: 1280px) {
    #playlist-container .sortable-ghost td:nth-last-child(5),
    #playlist-container .sortable-ghost td:nth-last-child(4),
    #playlist-container .sortable-ghost td:nth-last-child(-n + 2) {
        display: table-cell !important;
    }
}
</style>
