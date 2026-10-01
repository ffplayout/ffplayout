<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { useEventSource } from '@vueuse/core'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'
import { authFetch } from '@/composables/authFetch'

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
        const result = await configStore.setPlayoutConfig(configStore.playout)
        if (result.requires_restart) {
            const id = configStore.channels[configStore.i]?.id
            const status = await authFetch<string>(`/api/control/${id}/process`, {
                method: 'POST',
                headers: { ...configStore.contentType, ...authStore.authHeader },
                body: JSON.stringify({ command: 'status' }),
            })
            if (status === 'active') configStore.showRestartModal = true
        }
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
            <section class="grid gap-4 sm:grid-cols-2" aria-label="Program output audio meters">
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
                <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '1' }) }}</span>
            </fieldset>
            <fieldset class="fieldset mt-5 rounded-box w-full">
                <label class="fieldset">
                    <span class="fieldset-legend">Apply audio normalization to</span>
                    <select v-model="configStore.playout.audio.loudness_scope" class="select w-full">
                        <option value="all">All sources</option>
                        <option value="live">Live input only</option>
                        <option value="off">Off</option>
                    </select>
                </label>
                <p class="fieldset-label items-baseline">
                    Source selection takes effect after restarting playout. Other settings update immediately.
                </p>
            </fieldset>
            <div
                v-if="configStore.playout.audio.loudness_scope !== 'off'"
                class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
            >
                <label class="fieldset"
                    ><span class="fieldset-legend">Compression ratio</span>
                    <input
                        v-model.number="configStore.playout.audio.compressor_ratio"
                        type="number"
                        min="1"
                        max="10"
                        step="0.5"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '3:1' }) }}</span>
                </label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Pause threshold (RMS dBFS)</span>
                    <input
                        v-model.number="configStore.playout.audio.pause_threshold_dbfs"
                        type="number"
                        min="-90"
                        max="-20"
                        step="1"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '−55 dBFS' }) }}</span>
                </label>
                <label class="fieldset"
                    ><span class="fieldset-legend">Target LUFS</span
                    ><input
                        v-model.number="configStore.playout.audio.loudness_target_lufs"
                        type="number"
                        step="0.1"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{
                        t('config.audioDefaultValue', { value: '−23 LUFS' })
                    }}</span></label
                >
                <label class="fieldset"
                    ><span class="fieldset-legend">True peak ceiling (dBTP)</span
                    ><input
                        v-model.number="configStore.playout.audio.loudness_true_peak_ceiling_dbtp"
                        type="number"
                        max="0"
                        step="0.1"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '−1 dBTP' }) }}</span></label
                >
                <label class="fieldset"
                    ><span class="fieldset-legend">Maximum leveler gain (dB)</span
                    ><input
                        v-model.number="configStore.playout.audio.loudness_max_gain_db"
                        type="number"
                        min="0"
                        step="0.1"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '+8 dB' }) }}</span></label
                >
                <label class="fieldset"
                    ><span class="fieldset-legend">Maximum leveler attenuation (dB)</span
                    ><input
                        v-model.number="configStore.playout.audio.loudness_max_attenuation_db"
                        type="number"
                        max="0"
                        step="0.1"
                        class="input input-sm w-full"
                    />
                    <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '−12 dB' }) }}</span></label
                >
            </div>
            <details
                v-if="configStore.playout.audio.loudness_scope !== 'off'"
                class="collapse collapse-plus bg-base-100/40 border-2 border-base-100 mt-4"
            >
                <summary class="collapse-title font-semibold">{{ t('player.advanced') }}</summary>
                <div class="collapse-content">
                    <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                        <label class="fieldset"
                            ><span class="fieldset-legend">Compressor attack (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_attack_ms"
                                type="number"
                                min="0.1"
                                max="50.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '5 ms' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Compressor hold (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_hold_ms"
                                type="number"
                                min="0.0"
                                max="2000.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '100 ms' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Compressor release (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_release_ms"
                                type="number"
                                min="10.0"
                                max="10000.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '1200 ms' })
                            }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Release above 12 dB reduction (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_strong_release_ms"
                                type="number"
                                min="10.0"
                                max="10000.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '500 ms' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Compressor knee (dB)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_knee_db"
                                type="number"
                                min="0.0"
                                max="24.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '6 dB' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Pause detection hold (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.pause_hold_ms"
                                type="number"
                                min="0.0"
                                max="5000.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '300 ms' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Pause gain return delay (ms)</span>
                            <input
                                v-model.number="configStore.playout.audio.pause_return_delay_ms"
                                type="number"
                                min="0.0"
                                max="30000.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '2000 ms' })
                            }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Maximum output correction (dB)</span>
                            <input
                                v-model.number="configStore.playout.audio.loudness_output_max_correction_db"
                                type="number"
                                min="0.0"
                                max="12.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{ t('config.audioDefaultValue', { value: '±3 dB' }) }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Output correction up (dB/s)</span>
                            <input
                                v-model.number="configStore.playout.audio.loudness_output_gain_up_db_per_second"
                                type="number"
                                min="0.0"
                                max="5.0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '0.1 dB/s' })
                            }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Output correction down (dB/s)</span>
                            <input
                                v-model.number="configStore.playout.audio.loudness_output_gain_down_db_per_second"
                                type="number"
                                min="0.0"
                                max="5.0"
                                step="0.05"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '0.25 dB/s' })
                            }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Compressor threshold (dBFS)</span>
                            <input
                                v-model.number="configStore.playout.audio.compressor_threshold_dbfs"
                                type="number"
                                min="-60"
                                max="0"
                                step="1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '−26 dBFS' })
                            }}</span>
                        </label>
                        <label class="fieldset"
                            ><span class="fieldset-legend">Dead band (LU)</span
                            ><input
                                v-model.number="configStore.playout.audio.loudness_dead_band_lu"
                                type="number"
                                min="0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '1 LU' })
                            }}</span></label
                        >
                        <label class="fieldset"
                            ><span class="fieldset-legend">Input leveler gate (LUFS)</span
                            ><input
                                v-model.number="configStore.playout.audio.loudness_silence_gate_lufs"
                                type="number"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '−60 LUFS' })
                            }}</span></label
                        >
                        <label class="fieldset"
                            ><span class="fieldset-legend">Gain up (dB/s)</span
                            ><input
                                v-model.number="configStore.playout.audio.loudness_gain_up_db_per_second"
                                type="number"
                                min="0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '0.5 dB/s' })
                            }}</span></label
                        >
                        <label class="fieldset"
                            ><span class="fieldset-legend">Gain down (dB/s)</span
                            ><input
                                v-model.number="configStore.playout.audio.loudness_gain_down_db_per_second"
                                type="number"
                                min="0"
                                step="0.1"
                                class="input input-sm w-full"
                            />
                            <span class="fieldset-label">{{
                                t('config.audioDefaultValue', { value: '2 dB/s' })
                            }}</span></label
                        >
                    </div>
                </div>
            </details>
            <button class="btn btn-primary mt-6" type="submit">{{ t('config.save') }}</button>
        </form>
    </div>
    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>
