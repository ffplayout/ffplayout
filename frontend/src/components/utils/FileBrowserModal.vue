<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, shallowRef, useId, watch, watchEffect } from 'vue'
import { useI18n } from 'vue-i18n'
import { AuthFetchError } from '@/composables/authFetch'
import { stringFormatter } from '@/composables/helper'
import {
    formatBrowserSelection,
    type BrowserListing,
    type BrowserLoader,
    type SelectionMode,
    type SelectionPathFormat,
} from '@/composables/fileBrowser'

const props = withDefaults(
    defineProps<{
        show: boolean
        initialPath?: string
        mode?: SelectionMode
        pathFormat?: SelectionPathFormat
        extensions?: string[]
        title?: string
        load: BrowserLoader
    }>(),
    { initialPath: '', mode: 'file', pathFormat: 'relative' },
)
const emit = defineEmits<{ select: [path: string]; close: [] }>()
const { t } = useI18n()
const { fileSize } = stringFormatter()
const titleId = useId()
const dialog = ref<HTMLDialogElement>()
const listing = shallowRef<BrowserListing>()
const selected = ref('')
const loading = ref(false)
const error = ref('')
const notice = ref(false)
let request: AbortController | undefined

const selectionOptions = shallowRef({
    mode: props.mode,
    pathFormat: props.pathFormat,
    extensions: props.extensions?.slice(),
    title: props.title,
})

watchEffect(() => {
    // The parent can reset its field and filters as soon as it closes us.
    // Keep the last visible options throughout the dialog's closing animation.
    if (!props.show) return
    selectionOptions.value = {
        mode: props.mode,
        pathFormat: props.pathFormat,
        extensions: props.extensions?.slice(),
        title: props.title,
    }
})

const files = computed(() => {
    if (selectionOptions.value.mode === 'directory') return []
    const extensions = selectionOptions.value.extensions?.map((extension) => extension.replace(/^\./, '').toLowerCase())
    return (listing.value?.files ?? []).filter(
        (file) =>
            !extensions?.length || extensions.some((extension) => file.name.toLowerCase().endsWith(`.${extension}`)),
    )
})
const canSelectFile = computed(
    () => !loading.value && !error.value && files.value.some((file) => file.path === selected.value),
)
const canSelectDirectory = computed(
    () => selectionOptions.value.mode !== 'file' && Boolean(listing.value) && !loading.value && !error.value,
)

async function navigate(path: string, initial = false) {
    request?.abort()
    const controller = new AbortController()
    request = controller
    loading.value = true
    error.value = ''
    notice.value = false
    selected.value = ''
    try {
        let data: BrowserListing
        try {
            data = await props.load(path, initial, controller.signal)
        } catch (reason) {
            // An existing configuration may refer to a path outside this browser's
            // permitted root. Keep the input intact and start at the root instead.
            if (!initial || !path || !(reason instanceof AuthFetchError) || reason.response.status !== 403) throw reason
            data = await props.load('', false, controller.signal)
            if (!controller.signal.aborted) notice.value = true
        }
        if (controller.signal.aborted) return
        listing.value = data
        selected.value = files.value.some((file) => file.path === data.selected_path) ? data.selected_path! : ''
        notice.value ||= data.fallback
        await nextTick()
        dialog.value?.querySelector<HTMLElement>('[data-selected="true"]')?.scrollIntoView({ block: 'nearest' })
    } catch (reason) {
        if (!controller.signal.aborted) error.value = reason instanceof Error ? reason.message : String(reason)
    } finally {
        if (!controller.signal.aborted) loading.value = false
    }
}

function chooseFile(path = selected.value) {
    if (loading.value || error.value || !files.value.some((file) => file.path === path)) return
    confirmSelection(path)
}

function chooseDirectory() {
    if (!canSelectDirectory.value || !listing.value) return
    confirmSelection(listing.value.path)
}

function confirmSelection(path: string) {
    const root = listing.value?.breadcrumbs[0]?.path
    if (!root) return
    try {
        emit('select', formatBrowserSelection(path, root, selectionOptions.value.pathFormat))
        emit('close')
    } catch (reason) {
        error.value = reason instanceof Error ? reason.message : String(reason)
    }
}

watch(
    () => [props.show, props.initialPath, props.load] as const,
    async () => {
        request?.abort()
        if (!props.show) {
            dialog.value?.close()
            return
        }
        listing.value = undefined
        await nextTick()
        if (!props.show) return
        if (!dialog.value?.open) dialog.value?.showModal()
        void navigate(props.initialPath, true)
    },
    { immediate: true },
)

onBeforeUnmount(() => request?.abort())
</script>

<template>
    <Teleport to="body">
        <dialog
            ref="dialog"
            class="modal"
            :aria-labelledby="titleId"
            @cancel.prevent="emit('close')"
            @click.self="emit('close')"
        >
            <div class="modal-box w-3xl max-w-[95vw] flex flex-col max-h-[85vh]">
                <div class="flex items-center justify-between gap-3">
                    <h2 :id="titleId" class="font-bold text-lg">
                        {{ selectionOptions.title || t('fileBrowser.title') }}
                    </h2>
                    <button
                        type="button"
                        class="btn btn-sm btn-circle"
                        :aria-label="t('cancel')"
                        @click="emit('close')"
                    >
                        <i class="bi-x-lg" />
                    </button>
                </div>
                <nav v-if="listing" class="breadcrumbs text-sm shrink-0 my-2" :aria-label="t('fileBrowser.path')">
                    <ul>
                        <li v-for="crumb in listing.breadcrumbs" :key="crumb.path">
                            <button
                                type="button"
                                :disabled="loading || crumb.path === listing.path"
                                :aria-current="crumb.path === listing.path ? 'location' : undefined"
                                @click="navigate(crumb.path)"
                            >
                                <i class="bi-folder-fill me-1" />{{ crumb.name }}
                            </button>
                        </li>
                    </ul>
                </nav>
                <p v-if="notice" role="status" class="text-sm text-warning mb-2">{{ t('fileBrowser.fallback') }}</p>
                <p v-if="error" role="alert" class="text-error text-sm my-2">{{ error }}</p>
                <div
                    class="relative overflow-y-auto min-h-60 h-96 border border-base-content/20 rounded-sm"
                    :aria-busy="loading"
                >
                    <div
                        v-if="loading"
                        role="status"
                        class="absolute inset-0 z-10 flex justify-center items-center bg-base-100/80"
                    >
                        <span class="loading loading-spinner loading-lg" :aria-label="t('fileBrowser.loading')" />
                    </div>
                    <ul class="list-none text-sm" :aria-label="t('fileBrowser.entries')">
                        <li
                            v-for="folder in listing?.folders ?? []"
                            :key="folder.path"
                            class="border-b border-base-content/20 even:bg-base-200"
                        >
                            <button
                                type="button"
                                class="flex items-center gap-2 px-3 py-2 w-full text-start hover:bg-base-300 cursor-pointer"
                                :disabled="loading"
                                @click="navigate(folder.path)"
                            >
                                <i class="bi-folder-fill" /><span class="truncate">{{ folder.name }}</span>
                            </button>
                        </li>
                        <li
                            v-for="file in files"
                            :key="file.path"
                            class="border-b border-base-content/20 even:bg-base-200"
                        >
                            <button
                                type="button"
                                class="flex items-center gap-2 px-3 py-2 w-full text-start hover:bg-base-300 cursor-pointer"
                                :class="selected === file.path ? 'bg-secondary! text-secondary-content' : ''"
                                :disabled="loading"
                                :aria-pressed="selected === file.path"
                                :data-selected="selected === file.path"
                                @click="selected = file.path"
                                @dblclick="chooseFile(file.path)"
                                @keydown.enter.prevent="chooseFile(file.path)"
                            >
                                <i class="bi-file-earmark shrink-0" />
                                <span class="truncate min-w-0 flex-1">{{ file.name }}</span>
                                <span v-if="file.size !== undefined" class="shrink-0 text-xs tabular-nums opacity-70">
                                    {{ file.size === 0 ? '0 B' : fileSize(file.size) }}
                                </span>
                            </button>
                        </li>
                    </ul>
                    <p
                        v-if="listing && !loading && !error && !listing.folders.length && !files.length"
                        class="p-4 text-base-content/60"
                    >
                        {{ t('fileBrowser.empty') }}
                    </p>
                </div>
                <div class="text-sm my-3 break-all">
                    <span class="font-semibold">{{ t('fileBrowser.path') }}:</span>
                    {{ selected || listing?.path || '—' }}
                </div>
                <div class="flex flex-wrap justify-end join">
                    <button type="button" class="btn btn-sm join-item" @click="emit('close')">{{ t('cancel') }}</button>
                    <button
                        v-if="selectionOptions.mode !== 'file'"
                        type="button"
                        class="btn btn-sm btn-primary join-item"
                        :disabled="!canSelectDirectory"
                        @click="chooseDirectory"
                    >
                        {{ t('fileBrowser.selectDirectory') }}
                    </button>
                    <button
                        v-if="selectionOptions.mode !== 'directory'"
                        type="button"
                        class="btn btn-sm btn-primary join-item"
                        :disabled="!canSelectFile"
                        @click="chooseFile()"
                    >
                        {{ t('fileBrowser.select') }}
                    </button>
                </div>
            </div>
        </dialog>
    </Teleport>
</template>
