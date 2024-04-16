<template>
    <div v-if="mediaStore.isLoading" class="h-full w-full absolute z-10 flex justify-center bg-base-100/70">
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
            <button class="truncate" @click="mediaStore.getTree(`/${mediaStore.folderTree.source}/${folder.name}`)">
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
                        <i v-else-if="mediaType(element.name) === 'image'" class="bi-file-earmark-image" />
                        <i v-else class="bi-file-binary" />
                    </td>
                    <td class="px-[1px] py-1 truncate">
                        {{ element.name }}
                    </td>
                    <td class="px-1 py-1 w-[30px] text-center leading-3">
                        <button @click="preview(element.name)">
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
</template>
<script setup lang="ts">
const { width } = useWindowSize({ initialWidth: 800 })
const { secToHMS, mediaType } = stringFormatter()

const mediaStore = useMedia()
const { configID } = storeToRefs(useConfig())

const browserSortOptions = {
    group: { name: 'playlist', pull: 'clone', put: false },
    handle: '.grabbing',
    sort: false,
}

defineProps({
    preview: {
        type: Function,
        default() {
            return ''
        },
    },
})

onMounted(() => {
    if (!mediaStore.folderTree.parent) {
        mediaStore.getTree('')
    }
})

watch([configID], () => {
    mediaStore.getTree('')
})
</script>
