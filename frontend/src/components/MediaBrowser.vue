<template>
    <div class="h-full">
        <div v-if="mediaStore.isLoading" class="h-full w-full absolute z-10 flex justify-center bg-base-100/70">
            <span class="loading loading-spinner loading-lg" />
        </div>
        <div class="bg-base-100 border-b border-base-content/30">
            <div v-if="mediaStore.folderTree.parent && mediaStore.crumbs">
                <nav class="breadcrumbs px-2 py-[6px]">
                    <ul>
                        <li v-for="(crumb, index) in mediaStore.crumbs" :key="index">
                            <button
                                v-if="mediaStore.crumbs.length > 1 && mediaStore.crumbs.length - 1 > index"
                                class="flex items-center"
                                @click="mediaStore.getTree(crumb.path)"
                            >
                                <i class="bi-folder-fill me-1" />
                                <span class="text-xs font-bold text-base-content/60 leading-tight">
                                    {{ crumb.text }}
                                </span>
                            </button>
                            <div v-else class="flex items-center">
                                <i class="bi-folder-fill me-1" />
                                <span class="text-xs font-bold text-base-content/60">{{ crumb.text }}</span>
                            </div>
                        </li>
                    </ul>
                </nav>
            </div>
        </div>

        <div class="w-full h-[calc(100%-40px)]">
            <VirtualList
                id="mediaList"
                v-model="mediaStore.folderTree.files"
                class="w-full h-full"
                :handle="'.handle'"
                :group="dragGroup"
                :data-key="'name'"
                ghost-class="sortable-ghost"
                wrap-tag="ul"
                wrap-class="list-none text-sm"
                chosen-class="cursor-grabbing"
                placeholder-class="media-placeholder"
                :animation="50"
                :sortable="false"
            >
                <template #header>
                    <ul class="border-b border-base-content/20 list-none text-sm">
                        <li
                            v-for="folder in mediaStore.folderTree.folders"
                            :key="folder.uid"
                            class="grid grid-cols-[30px_auto] even:bg-base-200 py-1.5 items-center cursor-pointer"
                            @click="mediaStore.getTree(`/${mediaStore.folderTree.source}/${folder.name}`)"
                        >
                            <div class="px-2">
                                <i class="bi-folder-fill" />
                            </div>
                            <div class="truncate pe-1">
                                {{ folder.name }}
                            </div>
                        </li>
                    </ul>
                </template>
                <template #item="{ record, index }">
                    <li
                        :id="`file-${index}`"
                        :key="record.name"
                        class="grid grid-cols-[30px_auto_32px_62px] border-b border-base-content/20 py-1.5 items-center"
                        :class="mediaStore.folderTree.folders.length % 2 === 0 ? 'even:bg-base-200' : 'odd:bg-base-200'"
                    >
                        <div class="px-2" :class="{ timeHidden: configStore.playout.playlist.infinit }">
                            <i v-if="mediaType(record.name) === 'audio'" class="bi-music-note-beamed" />
                            <i v-else-if="mediaType(record.name) === 'video'" class="bi-film" />
                            <i v-else-if="mediaType(record.name) === 'image'" class="bi-file-earmark-image" />
                            <i v-else class="bi-file-binary" />
                        </div>
                        <div
                            class="truncate"
                            :class="{
                                'handle cursor-grab': width > 739 && configStore.playout.processing.mode === 'playlist',
                            }"
                        >
                            {{ record.name }}
                        </div>
                        <div class="text-center leading-3">
                            <button class="cursor-pointer" @click="preview(record.name)">
                                <i class="bi-play-fill" />
                            </button>
                        </div>
                        <div class="text-nowrap">
                            {{ secToHMS(record.duration) }}
                        </div>
                        <div class="hidden">00:00:00</div>
                        <div class="hidden">{{ secToHMS(record.duration) }}</div>
                        <div class="hidden">&nbsp;</div>
                        <div class="hidden">&nbsp;</div>
                        <div class="hidden">&nbsp;</div>
                    </li>
                </template>
            </VirtualList>
        </div>
    </div>
</template>
<script setup lang="ts">
import VirtualList from 'vue-virtual-draglist'

import { ref, onMounted, watch } from 'vue'
import { useWindowSize } from '@vueuse/core'
import { storeToRefs } from 'pinia'

import { stringFormatter } from '@/composables/helper'
import { useConfig } from '@/stores/config'
import { useMedia } from '@/stores/media'

const { width } = useWindowSize({ initialWidth: 800 })
const { secToHMS, mediaType } = stringFormatter()

const configStore = useConfig()
const mediaStore = useMedia()
const { i } = storeToRefs(useConfig())

const dragGroup = ref({ name: 'dragGroup', pull: 'clone', put: false })

defineProps({
    preview: {
        type: Function,
        default() {
            return ''
        },
    },
})

onMounted(async () => {
    if (!mediaStore.folderTree.parent || !mediaStore.currentPath) {
        await mediaStore.getTree('')
    }
})

watch([i], () => {
    mediaStore.getTree('')
})
</script>
