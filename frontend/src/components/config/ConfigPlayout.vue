<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import GenericModal from '@/components/utils/GenericModal.vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const logLevels = ['INFO', 'WARNING', 'ERROR']
const notificationLevels = ['INFO', 'WARNING', 'ERROR', 'FATAL']
const processingMode = ['folder', 'playlist']
const extensions = computed({
    get() {
        return configStore.playout.storage.extensions.join(',')
    },

    set(value: string) {
        configStore.playout.storage.extensions = value.replace(' ', '').split(/,|;/)
    },
})

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

async function onSubmitPlayout() {
    try {
        const update = await configStore.setPlayoutConfig(configStore.playout)
        configStore.onetimeInfo = true
        const { requires_restart: requiresRestart } = update

        indexStore.msgAlert('success', t('config.updatePlayoutSuccess'), 2)

        const id = configStore.channels[configStore.i]?.id
        const response = await authFetch<string>(`/api/control/${id}/process`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        })
        if (response === 'active' && requiresRestart) {
            configStore.showRestartModal = true
        }

        await configStore.getPlayoutConfig()
        await configStore.getPlayoutOutputs()
    } catch (error) {
        indexStore.msgAlert('error', error instanceof Error ? error.message : String(error), 3)
    }
}
</script>
<template>
    <div class="max-w-300 xs:pe-8">
        <h2 class="pt-3 text-3xl">{{ t('config.generalSettings') }}</h2>
        <form
            v-if="configStore.playout"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitPlayout"
        >
            <div class="text-xl pt-3 md:text-right">{{ t('config.general') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.generalHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Stop Threshold</legend>
                    <input
                        v-model="configStore.playout.general.stop_threshold"
                        type="number"
                        min="3"
                        class="input input-sm w-full max-w-36"
                    />
                    <p class="fieldset-label items-baseline">
                        {{ t('config.stopThreshold') }}
                    </p>
                </fieldset>
            </div>

            <template v-if="configStore.playout.mail.show">
                <div class="text-xl pt-3 md:text-right">{{ t('config.mail') }}:</div>
                <div class="md:pt-4">
                    <label class="form-control mb-2">
                        <div class="whitespace-pre-line">
                            {{ t('config.mailHelp') }}
                        </div>
                    </label>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Subject</legend>
                        <input
                            v-model="configStore.playout.mail.subject"
                            type="text"
                            name="subject"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Recipient</legend>
                        <input
                            v-model="configStore.playout.mail.recipient"
                            type="text"
                            name="recipient"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Mail Level</legend>
                        <select v-model="configStore.playout.mail.mail_level" class="select select-sm w-full max-w-xs">
                            <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">Interval</legend>
                        <input
                            v-model="configStore.playout.mail.interval"
                            type="number"
                            min="30"
                            step="10"
                            class="input input-sm w-full max-w-36"
                        />
                        <p class="fieldset-label items-baseline">{{ t('config.mailInterval') }}</p>
                    </fieldset>
                </div>
            </template>

            <template v-if="configStore.playout.notification.show">
                <div class="text-xl pt-3 md:text-right">{{ t('config.notification') }}:</div>
                <div class="md:pt-4">
                    <p class="mb-2 whitespace-pre-line">{{ t('config.notificationHelp') }}</p>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('config.notificationTopic') }}</legend>
                        <input
                            v-model="configStore.playout.notification.topic"
                            type="text"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('config.notificationLevel') }}</legend>
                        <select
                            v-model="configStore.playout.notification.level"
                            class="select select-sm w-full max-w-xs"
                        >
                            <option v-for="level in notificationLevels" :key="level" :value="level">{{ level }}</option>
                        </select>
                    </fieldset>
                    <fieldset class="fieldset">
                        <legend class="fieldset-legend">{{ t('config.notificationTags') }}</legend>
                        <input
                            v-model="configStore.playout.notification.tags"
                            type="text"
                            class="input input-sm w-full max-w-lg"
                        />
                    </fieldset>
                </div>
            </template>

            <div class="text-xl pt-3 md:text-right">{{ t('config.logging') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.logHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">ffmpeg Level</legend>
                    <select v-model="configStore.playout.logging.ffmpeg_level" class="select select-sm w-full max-w-xs">
                        <option v-for="level in logLevels" :key="level" :value="level">{{ level }}</option>
                    </select>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.logging.detect_silence" type="checkbox" class="checkbox" />
                        Detect Silence
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.logDetect') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Ignore Lines</legend>
                    <input v-model="formatIgnoreLines" type="text" class="input input-sm w-full max-w-full truncate" />
                    <p class="fieldset-label items-baseline">{{ t('config.logIgnore') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.processing') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.processingHelp') }}
                    </div>
                </label>
                <fieldset v-if="configStore.playout.program" class="fieldset grid gap-3 sm:grid-cols-3">
                    <legend class="fieldset-legend">{{ t('config.programFormat') }}</legend>
                    <label v-for="field in ['width', 'height', 'fps'] as const" :key="field" class="fieldset">
                        <span class="fieldset-legend">{{
                            t(field === 'fps' ? 'config.frameRate' : `config.${field}`)
                        }}</span>
                        <input
                            v-model.number="configStore.playout.program[field]"
                            type="number"
                            min="1"
                            step="1"
                            required
                            class="input input-sm w-full"
                        />
                    </label>
                    <p class="sm:col-span-3 text-sm">{{ t('config.programFormatHelp') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.mode') }}</legend>
                    <select v-model="configStore.playout.processing.mode" class="select select-sm w-full max-w-xs">
                        <option v-for="mode in processingMode" :key="mode" :value="mode">
                            {{ t(`config.processingModes.${mode}`) }}
                        </option>
                    </select>
                </fieldset>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.add_logo" type="checkbox" class="checkbox" />
                        Add Logo
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo</legend>
                    <input
                        v-model="configStore.playout.processing.logo"
                        type="text"
                        name="logo"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoPath') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Opacity</legend>
                    <input
                        v-model="configStore.playout.processing.logo_opacity"
                        type="number"
                        min="0"
                        max="1"
                        step="0.01"
                        class="input input-sm w-full max-w-36"
                    />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Scale</legend>
                    <input
                        v-model="configStore.playout.processing.logo_scale"
                        type="text"
                        name="logo_scale"
                        class="input input-sm w-full max-w-md"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoScale') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Logo Position</legend>
                    <input
                        v-model="configStore.playout.processing.logo_position"
                        type="text"
                        name="logo_position"
                        class="input input-sm w-full max-w-md"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingLogoPosition') }}</p>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.processing.vtt_enable" type="checkbox" class="checkbox" />
                        {{ t('config.enableVtt') }}
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.processingVTTEnable') }}</p>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">VTT Dummy</legend>
                    <input
                        v-model="configStore.playout.processing.vtt_dummy"
                        type="text"
                        name="vtt_dummy"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.processingVTTDummy') }}</p>
                </fieldset>

                <div v-if="configStore.playout.processing.vtt_enable" class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleName') }}</span>
                        <input
                            v-model.trim="configStore.playout.processing.vtt_name"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleLanguage') }}</span>
                        <input
                            v-model.trim="configStore.playout.processing.vtt_language"
                            type="text"
                            class="input input-sm w-full"
                        />
                    </label>
                    <label class="fieldset">
                        <span class="fieldset-legend">{{ t('config.hlsSubtitleDefault') }}</span>
                        <input v-model="configStore.playout.processing.vtt_default" type="checkbox" class="toggle" />
                    </label>
                </div>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.playlist') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.playlistHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Day Start</legend>
                    <input
                        v-model="configStore.playout.playlist.day_start"
                        type="text"
                        name="day_start"
                        class="input input-sm w-full max-w-xs"
                        pattern="(([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]|now|none)"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.playlistDayStart') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Length</legend>
                    <input
                        v-model="configStore.playout.playlist.length"
                        type="text"
                        name="length"
                        class="input input-sm w-full max-w-xs"
                        pattern="([01]?[0-9]|2[0-4]):[0-5][0-9]:[0-5][0-9]"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.playlistLength') }}</p>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.playlist.infinit" type="checkbox" class="checkbox" />
                        Infinit
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.playlistInfinit') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.storage') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.storageHelp') }}
                    </div>
                </label>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Filler</legend>
                    <input
                        v-model="configStore.playout.storage.filler"
                        type="text"
                        name="filler"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.storageFiller') }}</p>
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Extensions</legend>
                    <input v-model="extensions" type="text" name="extensions" class="input input-sm w-full max-w-lg" />
                    <p class="fieldset-label items-baseline">{{ t('config.storageExtension') }}</p>
                </fieldset>
                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.storage.shuffle" type="checkbox" class="checkbox" />
                        Shuffle
                    </label>
                    <p class="fieldset-label items-baseline">{{ t('config.storageShuffle') }}</p>
                </fieldset>
            </div>

            <div class="text-xl pt-3 md:text-right">{{ t('config.task') }}:</div>
            <div class="md:pt-4">
                <label class="form-control mb-2">
                    <div class="whitespace-pre-line">
                        {{ t('config.taskHelp') }}
                    </div>
                </label>

                <fieldset class="fieldset mt-2 rounded-box w-full">
                    <label class="fieldset-label text-base-content">
                        <input v-model="configStore.playout.task.enable" type="checkbox" class="checkbox" />
                        {{ t('config.enable') }}
                    </label>
                </fieldset>

                <fieldset class="fieldset">
                    <legend class="fieldset-legend">Path</legend>
                    <input
                        v-model="configStore.playout.task.path"
                        type="text"
                        name="task_path"
                        class="input input-sm w-full max-w-lg"
                    />
                    <p class="fieldset-label items-baseline">{{ t('config.taskPath') }}</p>
                </fieldset>
            </div>

            <div class="mt-5 mb-10">
                <button class="btn btn-primary" type="submit">{{ t('config.save') }}</button>
            </div>
        </form>
    </div>

    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="configStore.restart"
    />
</template>
