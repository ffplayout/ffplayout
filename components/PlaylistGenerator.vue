<template>
    <div
        class="z-50 fixed top-0 bottom-0 w-full h-full left-0 right-0 flex justify-center items-center bg-black/30 overflow-y-auto"
    >
        <div
            class="relative flex flex-col bg-base-100 w-[800px] min-w-[300px] max-w-[90vw] h-[680px] rounded-md p-5 shadow-xl"
        >
            <div class="font-bold text-lg">Generate Program</div>

            <div class="h-[calc(100%-95px)] mt-3">
                <div role="tablist" class="tabs tabs-bordered">
                    <input
                        type="radio"
                        name="my_tabs_2"
                        role="tab"
                        class="tab"
                        aria-label="Simple"
                        @change="advancedGenerator = false"
                        checked
                    />
                    <div role="tabpanel" class="tab-content w-full pt-3">
                        <div class="w-full">
                            <div class="grid">
                                <nav class="breadcrumbs px-3 pt-0">
                                    <ul>
                                        <li v-for="(crumb, index) in mediaStore.folderCrumbs" :key="index">
                                            <button
                                                v-if="
                                                    mediaStore.folderCrumbs.length > 1 &&
                                                    mediaStore.folderCrumbs.length - 1 > index
                                                "
                                                @click="mediaStore.getTree(crumb.path, true)"
                                            >
                                                <i class="bi-folder-fill me-1" />
                                                {{ crumb.text }}
                                            </button>
                                            <span v-else><i class="bi-folder-fill me-1" />{{ crumb.text }}</span>
                                        </li>
                                    </ul>
                                </nav>
                            </div>

                            <ul class="h-[475px] border border-my-gray rounded overflow-auto bg-base-300 m-1 py-1">
                                <li
                                    class="even:bg-base-200 px-2 w-full"
                                    v-for="folder in mediaStore.folderList.folders"
                                    :key="folder.uid"
                                >
                                    <div class="grid grid-cols-[auto_24px]">
                                        <button
                                            class="truncate text-left"
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
                                            <i class="bi-folder-fill" />
                                            {{ folder.name }}
                                        </button>
                                        <div v-if="!generateFromAll" class="text-center">
                                            <input
                                                class="checkbox checkbox-xs rounded"
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

                    <input
                        type="radio"
                        name="my_tabs_2"
                        role="tab"
                        class="tab"
                        aria-label="Advanced"
                        @change=";(advancedGenerator = true), resetCheckboxes()"
                    />
                    <div role="tabpanel" class="tab-content pt-3">
                        <div class="w-full">
                            <div class="grid grid-cols-[auto_48px] px-3 pt-0">
                                <nav class="breadcrumbs pt-0">
                                    <ul>
                                        <li v-for="(crumb, index) in mediaStore.folderCrumbs" :key="index">
                                            <button
                                                v-if="
                                                    mediaStore.folderCrumbs.length > 1 &&
                                                    mediaStore.folderCrumbs.length - 1 > index
                                                "
                                                @click="mediaStore.getTree(crumb.path, true)"
                                            >
                                                <i class="bi-folder-fill me-1" />
                                                {{ crumb.text }}
                                            </button>
                                            <span v-else><i class="bi-folder-fill me-1" />{{ crumb.text }}</span>
                                        </li>
                                    </ul>
                                </nav>
                                <div class="flex justify-end">
                                    <button
                                        type="button"
                                        class="btn btn-sm btn-primary"
                                        title="Add time block"
                                        @click="addTemplate()"
                                    >
                                        <i class="bi bi-folder-plus"></i>
                                    </button>
                                </div>
                            </div>
                            <div
                                class="h-[475px] border border-my-gray rounded grid bg-base-300 m-1"
                                :class="width < 740 ? 'grid-cols-1' : 'grid-cols-[300px_auto]'"
                            >
                                <Sortable
                                    :list="mediaStore.folderList.folders"
                                    :options="templateBrowserSortOptions"
                                    item-key="uid"
                                    class="overflow-auto py-1 border-my-gray"
                                    :class="width < 740 ? 'h-[240px] border-b' : 'border-e'"
                                    tag="ul"
                                >
                                    <template #item="{ element, index }">
                                        <li
                                            :id="`adv_folder_${index}`"
                                            class="even:bg-base-200 draggable px-2 w-full"
                                            :key="element.uid"
                                        >
                                            <button
                                                class="w-full truncate text-left"
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
                                                <i class="bi-folder-fill" />
                                                {{ element.name }}
                                            </button>
                                        </li>
                                    </template>
                                </Sortable>
                                <ul class="overflow-auto px-1 pb-1">
                                    <li
                                        v-for="item in template.sources"
                                        :key="item.start"
                                        class="flex flex-col gap-1 justify-center items-center border border-my-gray rounded mt-1 p-1"
                                    >
                                        <div class="grid grid-cols-[50px_67px_70px_67px_50px] join">
                                            <div
                                                class="input input-sm input-bordered join-item px-2 text-center bg-base-200"
                                            >
                                                Start:
                                            </div>
                                            <input
                                                type="text"
                                                class="input input-sm input-bordered join-item px-2 text-center"
                                                v-model="item.start"
                                            />
                                            <div
                                                class="input input-sm input-bordered join-item px-2 text-center bg-base-200"
                                            >
                                                Duration:
                                            </div>
                                            <input
                                                type="text"
                                                class="input input-sm input-bordered join-item px-2 text-center"
                                                v-model="item.duration"
                                            />
                                            <button
                                                class="btn btn-sm input-bordered join-item"
                                                :class="item.shuffle ? 'bg-base-100' : 'bg-base-300'"
                                                @click="item.shuffle = !item.shuffle"
                                            >
                                                {{ item.shuffle ? 'Shuffle' : 'Sorted' }}
                                            </button>
                                        </div>

                                        <Sortable
                                            :list="item.paths"
                                            item-key="index"
                                            class="w-full border border-my-gray rounded"
                                            :style="`height: ${item.paths ? item.paths.length * 23 + 31 : 300}px`"
                                            tag="ul"
                                            :options="templateTargetSortOptions"
                                            @add="addFolderToTemplate($event, item)"
                                        >
                                            <template #item="{ element, index }">
                                                <li
                                                    :id="`path_${index}`"
                                                    class="draggable grabbing py-0 even:bg-base-200 px-2"
                                                    :key="index"
                                                >
                                                    <i class="bi-folder-fill" />
                                                    {{ element.split(/[\\/]+/).pop() }}
                                                </li>
                                            </template>
                                        </Sortable>

                                        <div class="w-full flex justify-end">
                                            <button
                                                type="button"
                                                class="btn btn-sm bg-base-100"
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

            <div class="flex h-14 pt-6 justify-end items-center">
                <div v-if="!advancedGenerator" class="form-control">
                    <label class="label cursor-pointer w-12">
                        <span class="label-text">All</span>
                        <input
                            type="checkbox"
                            v-model="generateFromAll"
                            class="checkbox checkbox-xs rounded"
                            @change="resetCheckboxes()"
                        />
                    </label>
                </div>
                <div class="join ms-2">
                    <button
                        type="button"
                        class="btn btn-sm btn-primary join-item"
                        @click="resetCheckboxes(), resetTemplate(), close()"
                    >
                        Cancel
                    </button>
                    <button type="button" class="btn btn-sm btn-primary join-item" @click="generatePlaylist(), close()">
                        Ok
                    </button>
                </div>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
const { $dayjs } = useNuxtApp()

const { width } = useWindowSize({ initialWidth: 800 })
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

const { processPlaylist } = playlistOperations()

defineProps({
    close: {
        type: Function,
        default() {
            return ''
        },
    },
})

const advancedGenerator = ref(false)
const selectedFolders = ref([] as string[])
const generateFromAll = ref(false)
const template = ref({
    sources: [{
        start: configStore.configPlayout.playlist.day_start,
        duration: '02:00:00',
        shuffle: false,
        paths: [],
    }],
} as Template)

const templateBrowserSortOptions = {
    group: { name: 'folder', pull: 'clone', put: false },
    sort: false,
}
const templateTargetSortOptions = {
    group: 'folder',
    animation: 100,
    handle: '.grabbing',
}

async function generatePlaylist() {
    playlistStore.isLoading = true
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

    await $fetch(`/api/playlist/${configStore.configGui[configStore.configID].id}/generate/${playlistStore.listDate}`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body,
    })
        .then((response: any) => {
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                response.program,
                false
            )
            indexStore.msgAlert('success', 'Generate Playlist done...', 2)
        })
        .catch((e: any) => {
            indexStore.msgAlert('error', e.data ? e.data : e, 4)
        })

    // reset selections
    resetCheckboxes()
    resetTemplate()

    playlistStore.isLoading = false
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

function resetTemplate() {
    template.value.sources = []
}

function removeTemplate(item: TemplateItem) {
    const index = template.value.sources.indexOf(item)

    template.value.sources.splice(index, 1)
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
