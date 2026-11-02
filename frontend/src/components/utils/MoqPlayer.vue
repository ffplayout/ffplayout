<script setup lang="ts">
import { onMounted, onBeforeUnmount, computed, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type MoqWatch from '@moq/watch/element'
import { authFetch } from '@/composables/authFetch'
import type { moqDiagnostics } from '@/composables/moqDiagnostics'
import { refreshMoqTicketOnDisconnect } from '@/composables/moqTicketRefresh'
import { resumeMoqAudio } from '@/composables/moqAudioResume'
import { synchronizeMoqAudioLevel } from '@/composables/moqAudioLevelSync'
import { moqRelayUrl, type MoqBootstrap } from '@/composables/moqConnection'

const props = withDefaults(defineProps<{ channel: number; publicAccess?: boolean; audioMeter?: boolean }>(), {
    publicAccess: false,
})
const emit = defineEmits<{ audioLevel: [level: AudioLevel | null] }>()
const { t } = useI18n()
// Explicitly opt in before mounting; production and public playback stay quiet.
const diagnosticsEnabled =
    import.meta.env.DEV && !props.publicAccess && new URLSearchParams(location.search).get('diagnostics') === '1'
const host = ref<HTMLElement>()
const status = ref('')
const error = ref(false)
const connectionState = ref<'connecting' | 'waiting' | ''>('connecting')
const connectionMessage = computed(() =>
    error.value
        ? status.value
        : connectionState.value === 'waiting'
          ? t('config.moqWaiting')
          : connectionState.value === 'connecting'
            ? t('config.moqConnecting')
            : '',
)
const measurements = shallowRef<ReturnType<typeof moqDiagnostics>>()
const report = ref('')
let player: MoqWatch | undefined
let disposed = false

async function loadConfig(signal?: AbortSignal): Promise<MoqBootstrap> {
    const path = `${props.publicAccess ? '/moq/config' : '/api/moq/config'}?channel=${props.channel}`
    return props.publicAccess
        ? await fetch(path, { cache: 'no-store', signal }).then(async (response) => {
              if (!response.ok) throw new Error(t('config.moqPublicUnavailable'))
              return (await response.json()) as MoqBootstrap
          })
        : await authFetch<MoqBootstrap>(path, { signal })
}

async function connect(prefetched?: MoqBootstrap, signal?: AbortSignal) {
    if (disposed) return
    error.value = false
    try {
        const config = prefetched ?? (await loadConfig(signal))
        if (disposed || signal?.aborted || !player) return
        const relay = moqRelayUrl(config, props.channel)
        player.setAttribute('delay', `${config.delay_ms}ms`)
        player.name = config.broadcast
        player.url = relay
    } catch (reason) {
        if (!disposed && !signal?.aborted) {
            error.value = true
            status.value = reason instanceof Error ? reason.message : String(reason)
        }
        throw reason
    }
}

onMounted(async () => {
    status.value = t('config.moqLoading')
    try {
        const config = await loadConfig()
        if (disposed) return
        await Promise.all([import('@moq/watch/element'), import('@moq/watch/ui')])
        if (disposed || !host.value) return
        const ui = document.createElement('moq-watch-ui')
        // The library renders this button inside its shadow root without a CSS part.
        const buttonStyle = document.createElement('style')
        buttonStyle.textContent = `
            .watch-ui__offline-indicator { display: none !important; }
            .center-play {
                width: 3.5rem;
                height: 3.5rem;
                background: #313131;
            }
            .center-play svg { width: 1.5rem; height: 1.5rem; }
            .center-play:hover {
                background: #414141;
                box-shadow: 0 0.5rem 2rem #0008;
            }
        `
        ui.shadowRoot?.append(buttonStyle)
        player = document.createElement('moq-watch')
        const canvas = document.createElement('canvas')
        canvas.addEventListener('click', () => {
            if (player) player.paused = !player.paused
        })
        player.append(canvas)
        ui.append(player)
        host.value.append(ui)
        player.paused = true
        player.muted = false
        player.buffer = 100 as MoqWatch['buffer']
        player.signals.run((effect) => {
            const state = effect.get(player!.connection.status)
            const broadcast = effect.get(player!.broadcast.out.status)
            connectionState.value = state !== 'connected' ? 'connecting' : broadcast === 'offline' ? 'waiting' : ''
        })
        resumeMoqAudio(player.signals, player.audio.out.context, player.controls, document, window)
        if (diagnosticsEnabled) {
            const { moqDiagnostics } = await import('@/composables/moqDiagnostics')
            if (disposed) return
            measurements.value = moqDiagnostics(player)
            player.signals.run((effect) => {
                const failure = effect.get(player!.connection.error)
                const catalog = effect.get(player!.broadcast.out.catalog)
                const paused = effect.get(player!.controls.paused)
                error.value = Boolean(failure)
                status.value = failure
                    ? failure.message
                    : catalog
                      ? paused
                          ? t('config.moqPaused')
                          : t('config.moqPlaying')
                      : t('config.moqWaiting')
            })
        }
        if (props.audioMeter && !props.publicAccess) {
            synchronizeMoqAudioLevel(player, (level) => emit('audioLevel', level))
        }
        if (!props.publicAccess) {
            refreshMoqTicketOnDisconnect(player.signals, player.connection, (signal) => connect(undefined, signal))
        }
        await connect(config)
    } catch (reason) {
        error.value = true
        status.value = reason instanceof Error ? reason.message : String(reason)
    }
})

onBeforeUnmount(() => {
    disposed = true
    measurements.value?.close()
    if (player) {
        player.paused = true
        player.url = undefined
        player.signals.close()
    }
    host.value?.replaceChildren()
})

async function copyDiagnostics() {
    const text = measurements.value?.report() ?? ''
    try {
        await navigator.clipboard.writeText(text)
    } catch {
        report.value = text
    }
}
</script>
<template>
    <div class="moq-player relative w-full">
        <div class="relative">
            <div ref="host" class="w-full" />
            <div
                v-if="connectionMessage"
                role="status"
                aria-live="polite"
                class="absolute inset-0 flex items-center justify-center pointer-events-none p-4 text-center text-sm text-white"
            >
                <span class="rounded bg-black/70 px-3 py-2">{{ connectionMessage }}</span>
            </div>
        </div>
        <p v-if="diagnosticsEnabled && status" role="status" class="text-sm mt-1" :class="{ 'text-error': error }">
            {{ status }}
        </p>
        <details v-if="diagnosticsEnabled && measurements" class="text-sm mt-2">
            <summary class="cursor-pointer">{{ t('config.moqDiagnostics') }}</summary>
            <pre class="overflow-auto text-xs">{{ JSON.stringify(measurements.snapshot.value, null, 2) }}</pre>
            <button type="button" class="btn btn-xs" @click="copyDiagnostics">
                {{ t('config.moqCopyDiagnostics') }}
            </button>
            <textarea
                v-if="report"
                :value="report"
                readonly
                class="textarea w-full"
                :aria-label="t('config.moqDiagnostics')"
            />
        </details>
    </div>
</template>
<style scoped>
.moq-player :deep(moq-watch-ui),
.moq-player :deep(moq-watch) {
    display: block;
    width: 100%;
}
.moq-player :deep(canvas) {
    display: block;
    width: 100%;
    aspect-ratio: 16 / 9;
    background: #000;
}
</style>
