<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const config = useConfig()
const auth = useAuth()
const channel = computed(() => config.channels[config.i]?.id)
const cache = ref(128)
const loaded = ref(false)
const busy = ref(false)
const saved = ref(false)
const error = ref('')
let generation = 0

watch(
    channel,
    async (id, _, onCleanup) => {
        const version = ++generation
        const controller = new AbortController()
        onCleanup(() => {
            generation++
            controller.abort()
        })
        loaded.value = false
        busy.value = false
        saved.value = false
        error.value = ''
        if (id === undefined) return
        try {
            const actual = await authFetch<PlayoutConfigExt>(`/api/playout/config/${id}`, { signal: controller.signal })
            if (version !== generation) return
            cache.value = actual.processing.graphics_cache_mib
            loaded.value = true
        } catch (reason) {
            if (!controller.signal.aborted && version === generation)
                error.value = reason instanceof Error ? reason.message : String(reason)
        }
    },
    { immediate: true },
)

async function save() {
    if (!loaded.value || busy.value || !Number.isInteger(cache.value) || cache.value < 32 || cache.value > 1024) return
    const version = generation
    const id = channel.value
    const submitted = cache.value
    busy.value = true
    saved.value = false
    error.value = ''
    try {
        await authFetch(`/api/playout/config/${id}?scope=graphics_cache`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                ...config.playoutSaved,
                processing: { ...config.playoutSaved.processing, graphics_cache_mib: submitted },
            }),
        })
        if (version !== generation) return
        config.playoutSaved.processing.graphics_cache_mib = submitted
        config.playout.processing.graphics_cache_mib = submitted
        saved.value = true
    } catch (reason) {
        if (version === generation) error.value = reason instanceof Error ? reason.message : String(reason)
    } finally {
        if (version === generation) busy.value = false
    }
}
</script>

<template>
    <div class="bg-base-200 border border-base-content/30 rounded-box max-w-3xl mx-auto p-4">
        <p class="mb-4 text-sm">{{ t('graphicsPage.settingsHelp') }}</p>
        <form @submit.prevent="save">
            <p v-if="error" role="alert" class="text-error">{{ error }}</p>
            <p v-if="saved" role="status" class="text-success">{{ t('config.updatePlayoutSuccess') }}</p>
            <fieldset :disabled="!loaded || busy || auth.role === 'user'" class="grid gap-3">
                <label class="fieldset">
                    <span>{{ t('graphicsPage.animationCache') }}</span>
                    <input
                        v-model.number="cache"
                        type="number"
                        min="32"
                        max="1024"
                        step="1"
                        required
                        class="input input-sm w-36"
                    />
                    <span class="text-xs">{{ t('graphicsPage.animationCacheHelp') }}</span>
                </label>
                <button type="submit" class="btn btn-sm btn-primary w-fit">{{ t('config.save') }}</button>
            </fieldset>
        </form>
    </div>
</template>
