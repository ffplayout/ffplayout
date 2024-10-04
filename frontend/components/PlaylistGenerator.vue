<template>
    <div
        class="z-50 fixed inset-0 flex justify-center bg-black/30 overflow-auto py-5"
    >
        <div
            class="relative flex flex-col bg-base-100 w-[800px] min-w-[300px] max-w-[90vw] h-[680px] my-auto rounded-md p-5 shadow-xl"
        >
            <div class="font-bold text-lg">{{ t('player.generateProgram') }}</div>

            <div class="h-[calc(100%-95px)] mt-3">
                <div role="tablist" class="tabs tabs-bordered">
                    <input
                        type="radio"
                        name="my_tabs_2"
                        role="tab"
                        class="tab"
                        :aria-label="t('player.simple')"
                        checked
                        @change="advancedGenerator = false"
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
                                    v-for="folder in mediaStore.folderList.folders"
                                    :key="folder.uid"
                                    class="even:bg-base-200 px-2 w-full"
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
                        :aria-label="t('player.advanced')"
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
                                        :title="t('player.addBlock')"
                                        @click="addTemplate()"
                                    >
                                        <i class="bi bi-folder-plus" />
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
                                            :key="element.uid"
                                            class="even:bg-base-200 draggable px-2 w-full"
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
                                        <div class="flex flex-wrap xs:grid xs:grid-cols-[58px_64px_67px_64px_67px] xs:join">
                                            <div
                                                class="input input-sm input-bordered join-item px-1 text-center bg-base-200 leading-7"
                                            >
                                                {{ t('player.start') }}:
                                            </div>
                                            <input
                                                v-model="item.start"
                                                type="text"
                                                class="input input-sm input-bordered join-item px-1 text-center"
                                            />
                                            <div
                                                class="input input-sm input-bordered join-item px-1 text-center bg-base-200 leading-7"
                                            >
                                            {{ t('player.duration') }}:
                                            </div>
                                            <input
                                                v-model="item.duration"
                                                type="text"
                                                class="input input-sm input-bordered join-item px-1 text-center"
                                            />
                                            <button
                                                class="btn btn-sm input-bordered join-item"
                                                :class="item.shuffle ? 'bg-base-100' : 'bg-base-300'"
                                                @click="item.shuffle = !item.shuffle"
                                            >
                                                {{ item.shuffle ? t('player.shuffle') : t('player.sorted') }}
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
                                                    :key="index"
                                                    class="draggable grabbing py-0 even:bg-base-200 px-2"
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
                        <span class="label-text">{{ t('player.all') }}</span>
                        <input
                            v-model="generateFromAll"
                            type="checkbox"
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
                        {{ t('cancel') }}
                    </button>
                    <button type="button" class="btn btn-sm btn-primary join-item" @click="generatePlaylist(), close()">
                        {{ t('ok') }}
                    </button>
                </div>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
const { $dayjs } = useNuxtApp()
const { t } = useI18n()

const { width } = useWindowSize({ initialWidth: 800 })
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

const { processPlaylist } = playlistOperations()

const prop = defineProps({
    close: {
        type: Function,
        default() {
            return ''
        },
    },
    switchClass: {
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
    sources: [
        {
            start: configStore.playout.playlist.day_start,
            duration: '02:00:00',
            shuffle: false,
            paths: [],
        },
    ],
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
    const checkboxes = document.getElementsByClassName('folder-check') as HTMLCollectionOf<HTMLInputElement>

    if (checkboxes) {
        for (const box of checkboxes) {
            box.checked = false
        }
    }
}

function addFolderToTemplate(event: any, item: TemplateItem) {
    const o = event.oldIndex
    const n = event.newIndex

    event.item.remove()

    const storagePath = configStore.channels[configStore.i].storage
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
    let start = $dayjs('2000-01-01T00:00:00')

    if (last) {
        const t = $dayjs(`2000-01-01T${last.duration}`)
        start = $dayjs(`2000-01-01T${last.start}`)
            .add(t.hour(), 'hour')
            .add(t.minute(), 'minute')
            .add(t.second(), 'second')
    }

    template.value.sources.push({
        start: start.format('HH:mm:ss'),
        duration: '02:00:00',
        shuffle: false,
        paths: [],
    })
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

    await $fetch(`/api/playlist/${configStore.channels[configStore.i].id}/generate/${playlistStore.listDate}`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body,
    })
        .then((response: any) => {
            playlistStore.playlist = processPlaylist(playlistStore.listDate, response.program, false)
            prop.switchClass()
            indexStore.msgAlert('success', t('player.generateDone'), 2)
        })
        .catch((e: any) => {
            indexStore.msgAlert('error', e.data ? e.data : e, 4)
        })

    // reset selections
    resetCheckboxes()
    resetTemplate()

    playlistStore.isLoading = false
}
</script>
