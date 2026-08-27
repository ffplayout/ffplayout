<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { useEventSource } from '@vueuse/core'
import { useI18n } from 'vue-i18n'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const audioLevel = ref<AudioLevel | null>(null)
const loudness = ref<LiveLoudnessMetrics | null>(null)
const streamUrl = ref(
    `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&audio_meter=true&uuid=${authStore.uuid}`,
)

const { data, close } = useEventSource(streamUrl, [], {
    autoReconnect: { retries: -1, delay: 1000 },
})

watch(data, () => {
    if (!data.value) return

    try {
        const status = JSON.parse(data.value) as PlayoutStatus
        audioLevel.value = status.audio ?? null
        loudness.value = status.loudness ?? null
    } catch {
        // The connection banner and keep-alive messages are not status JSON.
    }
})

watch(
    () => configStore.i,
    () => {
        audioLevel.value = null
        loudness.value = null
        streamUrl.value = `/data/event/${configStore.channels[configStore.i]?.id}?endpoint=playout&audio_meter=true&uuid=${authStore.uuid}`
    },
)

onBeforeUnmount(close)

function meterPercent(value: number | null | undefined, floor = -60) {
    if (value == null) return 0
    return Math.min(100, Math.max(0, ((value - floor) / -floor) * 100))
}

function meterValue(value: number | null | undefined, unit: string) {
    return value == null ? '—' : `${value.toFixed(1)} ${unit}`
}

async function saveAudio() {
    try {
        await configStore.setPlayoutConfig(configStore.playout)
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
        return
    }

    indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)
    await configStore.getPlayoutConfig()
}
</script>

<template>
    <div class="max-w-300 xs:pe-8">
        <h2 class="pt-3 text-3xl">Audio</h2>
        <form v-if="configStore.playout" class="mt-10 max-w-3xl" @submit.prevent="saveAudio">
            <section class="grid gap-4 sm:grid-cols-2" aria-label="Live audio meters">
                <div class="rounded-box border border-base-300 bg-base-200 p-4">
                    <div class="flex items-baseline justify-between gap-3">
                        <h3 class="font-semibold">Volume meter</h3>
                        <span class="font-mono text-sm">{{ meterValue(audioLevel?.peak_db, 'dBFS') }}</span>
                    </div>
                    <div class="mt-3 h-3 overflow-hidden rounded-full bg-base-300">
                        <div
                            class="h-full bg-success transition-[width] duration-300"
                            :style="{ width: `${meterPercent(audioLevel?.peak_db)}%` }"
                        />
                    </div>
                    <p class="mt-2 text-sm text-base-content/70">RMS: {{ meterValue(audioLevel?.rms_db, 'dBFS') }}</p>
                </div>
                <div class="rounded-box border border-base-300 bg-base-200 p-4">
                    <div class="flex items-baseline justify-between gap-3">
                        <h3 class="font-semibold">Loudness meter</h3>
                        <span class="font-mono text-sm">{{ meterValue(loudness?.short_term_lufs, 'LUFS') }}</span>
                    </div>
                    <div class="mt-3 h-3 overflow-hidden rounded-full bg-base-300">
                        <div
                            class="h-full bg-success transition-[width] duration-300"
                            :style="{ width: `${meterPercent(loudness?.short_term_lufs)}%` }"
                        />
                    </div>
                    <p class="mt-2 text-sm text-base-content/70">
                        Integrated: {{ meterValue(loudness?.integrated_lufs, 'LUFS') }} · True peak:
                        {{ meterValue(loudness?.true_peak_dbtp, 'dBTP') }}
                    </p>
                </div>
            </section>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">Volume</legend>
                <input
                    v-model.number="configStore.playout.audio.volume"
                    type="number"
                    min="0"
                    max="1.5"
                    step="0.001"
                    class="input input-sm w-36"
                />
            </fieldset>
            <fieldset class="fieldset mt-5 rounded-box w-full">
                <label class="fieldset-label text-base-content">
                    <input v-model="configStore.playout.audio.live_loudness_enable" type="checkbox" class="checkbox" />
                    Live ingest loudness normalization (EBU R128)
                </label>
                <p class="fieldset-label items-baseline">
                    Changes take effect immediately; only live ingest is processed.
                </p>
            </fieldset>
            <div v-if="configStore.playout.audio.live_loudness_enable" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                <label class="fieldset"
                    ><span class="fieldset-legend">Target LUFS</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_target_lufs"
                        type="number"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Dead band (LU)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_dead_band_lu"
                        type="number"
                        min="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">True peak ceiling (dBTP)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_true_peak_ceiling_dbtp"
                        type="number"
                        max="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Maximum gain (dB)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_max_gain_db"
                        type="number"
                        min="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Maximum attenuation (dB)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_max_attenuation_db"
                        type="number"
                        max="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Silence gate (LUFS)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_silence_gate_lufs"
                        type="number"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Gain up (dB/s)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_gain_up_db_per_second"
                        type="number"
                        min="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Gain down (dB/s)</span
                    ><input
                        v-model.number="configStore.playout.audio.live_loudness_gain_down_db_per_second"
                        type="number"
                        min="0"
                        step="0.1"
                        class="input input-sm w-full"
                /></label>
            </div>
            <button class="btn btn-primary mt-6" type="submit">{{ t('config.save') }}</button>
        </form>
    </div>
</template>
