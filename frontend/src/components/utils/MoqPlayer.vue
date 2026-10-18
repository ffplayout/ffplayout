<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, shallowRef } from 'vue'
import { useI18n } from 'vue-i18n'
import type MoqWatch from '@moq/watch/element'
import { authFetch } from '@/composables/authFetch'
import type { moqDiagnostics } from '@/composables/moqDiagnostics'
import { refreshMoqTicketOnFailure } from '@/composables/moqTicketRefresh'

const props = withDefaults(defineProps<{ channel: number; publicAccess?: boolean }>(), { publicAccess: false })
const { t } = useI18n()
// Explicitly opt in before mounting; production and public playback stay quiet.
const diagnosticsEnabled =
    import.meta.env.DEV && !props.publicAccess && new URLSearchParams(location.search).get('diagnostics') === '1'
const host = ref<HTMLElement>()
const status = ref('')
const error = ref(false)
const measurements = shallowRef<ReturnType<typeof moqDiagnostics>>()
const report = ref('')
let player: MoqWatch | undefined
let disposed = false
let connecting = false

interface Bootstrap {
    broadcast: string
    relay_url: string | null
    relay_port: number
    delay_ms: number
    ticket: string | null
}

async function loadConfig(signal?: AbortSignal): Promise<Bootstrap> {
    const path = `${props.publicAccess ? '/moq/config' : '/api/moq/config'}?channel=${props.channel}`
    return props.publicAccess
        ? await fetch(path, { cache: 'no-store', signal }).then(async (response) => {
              if (!response.ok) throw new Error(t('config.moqPublicUnavailable'))
              return (await response.json()) as Bootstrap
          })
        : await authFetch<Bootstrap>(path, { signal })
}

async function connect(prefetched?: Bootstrap, signal?: AbortSignal) {
    if (disposed || connecting) return
    connecting = true
    error.value = false
    try {
        const config = prefetched ?? (await loadConfig(signal))
        if (disposed || signal?.aborted || !player) return
        const relay = new URL('/moq/relay', location.href)
        relay.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
        // Development uses Vite's same-origin WebSocket proxy, including when
        // the backend advertises a direct relay address on another host/port.
        if (!import.meta.env.DEV) {
            if (config.relay_url) relay.href = config.relay_url
            else if (location.protocol === 'http:' && ['localhost', '127.0.0.1', '[::1]'].includes(location.hostname)) {
                relay.href = `http://${location.hostname}:${config.relay_port}/channels`
            }
        }
        relay.searchParams.set('channel', String(props.channel))
        if (config.ticket) relay.searchParams.set('ticket', config.ticket)
        player.setAttribute('delay', `${config.delay_ms}ms`)
        player.name = config.broadcast
        player.url = relay
    } catch (reason) {
        if (!disposed && !signal?.aborted) {
            error.value = true
            status.value = reason instanceof Error ? reason.message : String(reason)
        }
        throw reason
    } finally {
        connecting = false
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
        if (!props.publicAccess) {
            refreshMoqTicketOnFailure(player.signals, player.connection, (signal) => connect(undefined, signal))
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
    <div class="moq-player w-full">
        <div ref="host" class="w-full" />
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
