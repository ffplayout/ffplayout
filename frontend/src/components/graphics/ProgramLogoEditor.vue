<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { useI18n } from 'vue-i18n'
import { useGraphicMetadata } from '@/composables/graphicMetadata'
import GraphicSourceInfo from './GraphicSourceInfo.vue'
import { authFetch } from '@/composables/authFetch'
import { browseChannelPaths } from '@/composables/fileBrowser'
import { useConfig } from '@/stores/config'
import { useAuth } from '@/stores/auth'
import GenericModal from '@/components/utils/GenericModal.vue'
import FileBrowserModal from '@/components/utils/FileBrowserModal.vue'

const { t } = useI18n()
const config = useConfig()
const auth = useAuth()
const channel = computed(() => config.channels[config.i]?.id)
const form = ref(cloneDeep(config.playoutSaved.processing))
const mediaInfo = useGraphicMetadata(
    () => channel.value,
    () => form.value.logo,
)
const browsing = ref(false)
const busy = ref(false)
const loaded = ref(false)
const error = ref('')
const saved = ref(false)
let generation = 0
watch(
    channel,
    async (id, _, onCleanup) => {
        generation++
        const version = generation
        const controller = new AbortController()
        onCleanup(() => controller.abort())
        browsing.value = false
        error.value = ''
        saved.value = false
        loaded.value = false
        busy.value = true
        try {
            const actual = await authFetch<PlayoutConfigExt>(`/api/playout/config/${id}`, { signal: controller.signal })
            if (!controller.signal.aborted && version === generation) {
                form.value = cloneDeep(actual.processing)
                loaded.value = true
            }
        } catch (reason) {
            if (!controller.signal.aborted && version === generation)
                error.value = reason instanceof Error ? reason.message : String(reason)
        } finally {
            if (!controller.signal.aborted && version === generation) busy.value = false
        }
    },
    { immediate: true },
)

function load(path: string, initial: boolean, signal: AbortSignal) {
    return browseChannelPaths(channel.value!, path, initial, signal)
}

async function save() {
    if (!loaded.value || busy.value) return
    const version = generation
    const id = channel.value
    busy.value = true
    error.value = ''
    saved.value = false
    try {
        const result = await authFetch<{ requires_restart: boolean }>(`/api/playout/config/${id}?scope=logo`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ...config.playoutSaved, processing: form.value }),
        })
        if (version !== generation) return
        const actual = await authFetch<PlayoutConfigExt>(`/api/playout/config/${id}`)
        if (version !== generation) return
        const keys = ['add_logo', 'logo', 'logo_scale', 'logo_opacity', 'logo_position'] as const
        for (const key of keys) {
            Object.assign(config.playoutSaved.processing, { [key]: actual.processing[key] })
            Object.assign(config.playout.processing, { [key]: actual.processing[key] })
        }
        form.value = cloneDeep(actual.processing)
        saved.value = true
        if (result.requires_restart) {
            const status = await authFetch<string>(`/api/control/${id}/process`, {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ command: 'status' }),
            })
            if (version === generation && status === 'active') config.showRestartModal = true
        }
    } catch (reason) {
        if (version === generation) error.value = reason instanceof Error ? reason.message : String(reason)
    } finally {
        if (version === generation) busy.value = false
    }
}
onBeforeUnmount(() => {
    generation++
    config.showRestartModal = false
})
</script>

<template>
    <form class="max-w-3xl mx-auto" @submit.prevent="save">
        <p class="mb-4">{{ t('graphicsPage.logoHelp') }}</p>
        <p v-if="error" role="alert" class="text-error">{{ error }}</p>
        <p v-if="saved" role="status" class="text-success">{{ t('config.updatePlayoutSuccess') }}</p>
        <fieldset :disabled="busy || !loaded || auth.role === 'user'" class="grid gap-4">
            <label class="flex gap-2 items-center"
                ><input v-model="form.add_logo" type="checkbox" class="toggle" />{{
                    t('graphicsPage.enableLogo')
                }}</label
            >
            <label class="fieldset"
                ><span>{{ t('graphicsEditor.source') }}</span>
                <span class="join w-full"
                    ><input v-model="form.logo" class="input input-sm join-item w-full" /><button
                        type="button"
                        class="btn btn-sm join-item"
                        :aria-label="t('fileBrowser.title')"
                        @click="browsing = true"
                    >
                        <i class="bi-folder2-open" /></button
                ></span>
                <GraphicSourceInfo :info="mediaInfo" />
                <span class="text-xs">{{ t('config.processingLogoPath') }}</span>
            </label>
            <label class="fieldset"
                ><span>{{ t('graphicsEditor.opacity') }}</span
                ><input
                    v-model.number="form.logo_opacity"
                    type="number"
                    min="0"
                    max="1"
                    step="0.01"
                    required
                    class="input input-sm w-36"
            /></label>
            <label class="fieldset"
                ><span>{{ t('graphicsPage.logoScale') }}</span
                ><input v-model="form.logo_scale" class="input input-sm w-full" /><span class="text-xs">{{
                    t('config.processingLogoScale')
                }}</span></label
            >
            <label class="fieldset"
                ><span>{{ t('graphicsEditor.position') }}</span
                ><input v-model="form.logo_position" class="input input-sm w-full" /><span class="text-xs">{{
                    t('config.processingLogoPosition')
                }}</span></label
            >
            <button class="btn btn-sm btn-primary w-fit" type="submit">{{ t('config.save') }}</button>
        </fieldset>
        <GenericModal
            :title="t('config.restartTile')"
            :text="t('config.restartText')"
            :show="config.showRestartModal"
            :modal-action="config.restart"
        />
        <FileBrowserModal
            :show="browsing"
            :initial-path="form.logo"
            :load="load"
            path-format="relative"
            :extensions="[
                'png',
                'jpg',
                'jpeg',
                'webp',
                'bmp',
                'gif',
                'tif',
                'tiff',
                'avif',
                'ico',
                'mov',
                'mp4',
                'mkv',
                'webm',
                'avi',
            ]"
            @select="form.logo = $event"
            @close="browsing = false"
        />
    </form>
</template>
