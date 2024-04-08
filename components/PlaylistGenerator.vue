<template>
    <div class="z-50 fixed top-0 bottom-0 left-0 right-0 flex justify-center items-center bg-black/30">
        <div
            class="flex flex-col bg-base-100 w-[800px] min-w-[400px] max-w-[90%] h-auto min-h-[600px] max-h-[80%] rounded-md p-5 shadow-xl"
        >
            <div class="font-bold text-lg">Generate Program</div>

            <div class="h-[600px] mt-3">
                <div role="tablist" class="tabs tabs-lifted h-full">
                    <input
                        type="radio"
                        name="my_tabs_2"
                        role="tab"
                        class="tab"
                        aria-label="Simple"
                        @change="advancedGenerator = false"
                        checked
                    />
                    <div role="tabpanel" class="tab-content bg-base-100 border-base-300 rounded-box p-4">
                        <div class="h-full">
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
                            <ul class="bg-base-300 h-[500px] overflow-auto m-1 py-1">
                                <li
                                    class="list-group-item browser-item even:bg-base-200 px-2"
                                    v-for="folder in mediaStore.folderList.folders"
                                    :key="folder.uid"
                                >
                                    <div class="grid grid-cols-[auto_40px]">
                                        <div class="col browser-item-text">
                                            <button
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
                                        </div>
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
                    <div role="tabpanel" class="tab-content bg-base-100 border-base-300 rounded-box p-6">
                        <div>
                            <div class="row">
                                <div class="col col-10">
                                    <nav aria-label="breadcrumb">
                                        <ol class="breadcrumb border-0">
                                            <li
                                                class="breadcrumb-item"
                                                v-for="(crumb, index) in mediaStore.folderCrumbs"
                                                :key="index"
                                                :active="index === mediaStore.folderCrumbs.length - 1"
                                                @click.prevent="mediaStore.getTree(crumb.path, true)"
                                            >
                                                <a
                                                    v-if="
                                                        mediaStore.folderCrumbs.length > 1 &&
                                                        mediaStore.folderCrumbs.length - 1 > index
                                                    "
                                                    href="#"
                                                >
                                                    {{ crumb.text }}
                                                </a>
                                                <span v-else>{{ crumb.text }}</span>
                                            </li>
                                        </ol>
                                    </nav>
                                </div>
                                <div class="col d-flex justify-content-end">
                                    <button type="button" class="btn btn-primary p-2 py-0 m-1" @click="addTemplate()">
                                        <i class="bi bi-folder-plus"></i>
                                    </button>
                                </div>
                            </div>
                            <div class="row">
                                <div class="col col-5 browser-col">
                                    <Sortable
                                        :list="mediaStore.folderList.folders"
                                        :options="templateBrowserSortOptions"
                                        item-key="uid"
                                        class="list-group media-browser-scroll browser-div"
                                        tag="ul"
                                    >
                                        <template #item="{ element, index }">
                                            <li
                                                :id="`adv_folder_${index}`"
                                                class="draggable list-group-item browser-item"
                                                :key="element.uid"
                                            >
                                                <div class="row">
                                                    <div class="col-1 browser-icons-col">
                                                        <i class="bi-folder-fill browser-icons" />
                                                    </div>
                                                    <div class="col browser-item-text">
                                                        <a
                                                            class="link-light"
                                                            href="#"
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
                                                            {{ element.name }}
                                                        </a>
                                                    </div>
                                                </div>
                                            </li>
                                        </template>
                                    </Sortable>
                                </div>
                                <div class="col template-col">
                                    <ul class="list-group media-browser-scroll">
                                        <li v-for="item in template.sources" :key="item.start" class="list-group-item">
                                            <div class="input-group mb-3">
                                                <span class="input-group-text">Start</span>
                                                <input
                                                    type="test"
                                                    class="form-control"
                                                    aria-label="Start"
                                                    v-model="item.start"
                                                />
                                                <span class="input-group-text">Duration</span>
                                                <input
                                                    type="test"
                                                    class="form-control"
                                                    aria-label="Duration"
                                                    v-model="item.duration"
                                                />
                                                <input
                                                    type="checkbox"
                                                    class="btn-check"
                                                    :id="`shuffle-${item.start}`"
                                                    autocomplete="off"
                                                    v-model="item.shuffle"
                                                />
                                                <label class="btn btn-outline-primary" :for="`shuffle-${item.start}`">
                                                    Shuffle
                                                </label>
                                            </div>

                                            <Sortable
                                                :list="item.paths"
                                                item-key="index"
                                                class="list-group w-100 border"
                                                :style="`height: ${item.paths ? item.paths.length * 23 + 31 : 300}px`"
                                                tag="ul"
                                                :options="templateTargetSortOptions"
                                                @add="addFolderToTemplate($event, item)"
                                            >
                                                <template #item="{ element, index }">
                                                    <li
                                                        :id="`path_${index}`"
                                                        class="draggable grabbing list-group-item py-0"
                                                        :key="index"
                                                    >
                                                        {{ element.split(/[\\/]+/).pop() }}
                                                    </li>
                                                </template>
                                            </Sortable>

                                            <div class="col d-flex justify-content-end">
                                                <button
                                                    type="button"
                                                    class="btn btn-primary p-2 py-0 m-1"
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
            </div>

            <div class="flex h-14 pt-6 justify-end items-center">
                <div v-if="!advancedGenerator" class="form-control">
                    <label class="label cursor-pointer w-12">
                        <span class="label-text">All</span>
                        <input type="checkbox" v-model="generateFromAll" class="checkbox checkbox-xs rounded" @change="resetCheckboxes()" />
                    </label>
                </div>
                <div class="join ms-2">
                    <button
                        type="button"
                        class="btn btn-sm btn-primary join-item"
                        data-bs-dismiss="modal"
                        @click="resetCheckboxes(), resetTemplate(), close()"
                    >
                        Cancel
                    </button>
                    <button
                        type="button"
                        class="btn btn-sm btn-primary join-item"
                        data-bs-dismiss="modal"
                        @click="generatePlaylist(), close()"
                    >
                        Ok
                    </button>
                </div>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const mediaStore = useMedia()
const playlistStore = usePlaylist()

const { processPlaylist } = playlistOperations()
const contentType = { 'content-type': 'application/json;charset=UTF-8' }

defineProps({
    close: {
        type: Function,
        default() {
            return ''
        },
    },
})

const advancedGenerator = ref(false)
const playlistIsLoading = ref(false)
const selectedFolders = ref([] as string[])
const generateFromAll = ref(false)
const template = ref({
    sources: [],
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
    playlistIsLoading.value = true
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
        headers: { ...contentType, ...authStore.authHeader },
        body,
    })
        .then((response: any) => {
            playlistStore.playlist = processPlaylist(
                configStore.startInSec,
                configStore.playlistLength,
                response.program,
                false
            )
            indexStore.msgAlert('alert-success', 'Generate Playlist done...', 2)
        })
        .catch((e: any) => {
            indexStore.msgAlert('alert-error', e.data ? e.data : e, 4)
        })

    // reset selections
    resetCheckboxes()
    resetTemplate()

    playlistIsLoading.value = false
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
