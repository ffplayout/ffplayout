<template>
    <div id="playlist-container" class="relative w-full h-full !bg-base-300 rounded-e overflow-auto">
        <div v-if="playlistStore.isLoading" class="w-full h-full absolute z-10 flex justify-center bg-base-100/70">
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
                        :key="element.uid"
                        class="draggable border-t border-b border-base-content/20 duration-1000 transition-all"
                        :class="{
                            '!bg-lime-500/30':
                                playlistStore.playoutIsRunning &&
                                listDate === todayDate &&
                                index === playlistStore.currentClipIndex,
                        }"
                    >
                        <td class="ps-4 py-2 text-left">{{ secondsToTime(element.begin) }}</td>
                        <td class="py-2 text-left truncate" :class="{ 'grabbing cursor-grab': width > 768 }">
                            {{ filename(element.source) }}
                        </td>
                        <td class="py-2 text-center hover:text-base-content/70">
                            <button @click="preview(element.source)">
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
                                :checked="element.category && element.category === 'advertisement' ? true : false"
                                @change="setCategory($event, element)"
                            >
                        </td>
                        <td class="py-2 text-center hover:text-base-content/70">
                            <button @click="editItem(index)">
                                <i class="bi-pencil-square" />
                            </button>
                        </td>
                        <td class="py-2 text-center hidden xl:table-cell justify-center hover:text-base-content/70">
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

const { $dayjs } = useNuxtApp()
const { width } = useWindowSize({ initialWidth: 800 })

const configStore = useConfig()
const mediaStore = useMedia()
const playlistStore = usePlaylist()
const { secToHMS, filename, secondsToTime } = stringFormatter()
const { processPlaylist, genUID } = playlistOperations()

const todayDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const { listDate } = storeToRefs(usePlaylist())

const playlistSortOptions = {
    group: 'playlist',
    animation: 100,
    handle: '.grabbing',
}

const props = defineProps({
    getPlaylist: {
        type: Function,
        default() {
            return ''
        },
    },
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
    props.getPlaylist()
})

watch([listDate], async () => {
    await props.getPlaylist()
})

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

function deletePlaylistItem(index: number) {
    playlistStore.playlist.splice(index, 1)
}
</script>
