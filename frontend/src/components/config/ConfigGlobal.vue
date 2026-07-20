<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'
import { useI18n } from 'vue-i18n'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

import GenericModal from '@/components/utils/GenericModal.vue'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const settings = ref({} as GlobalSettings)
const savedSettings = ref({} as GlobalSettings)
const smtpPassword = ref('')
const loading = ref(true)

onMounted(getSettings)

async function getSettings() {
    loading.value = true

    try {
        const response = await fetch('/api/global', {
            headers: authStore.authHeader,
        })

        if (!response.ok) {
            throw new Error(await response.text())
        }

        settings.value = await response.json()
        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
    } catch {
        indexStore.msgAlert('error', t('config.updateGlobalFailed'), 3)
    } finally {
        loading.value = false
    }
}

function isChanged() {
    return smtpPassword.value.length > 0 || !isEqual(settings.value, savedSettings.value)
}

async function save() {
    try {
        const response = await fetch('/api/global', {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                smtp_server: settings.value.smtp_server,
                smtp_user: settings.value.smtp_user,
                smtp_password: smtpPassword.value,
                smtp_starttls: settings.value.smtp_starttls,
                smtp_port: settings.value.smtp_port,
            }),
        })

        if (!response.ok) {
            throw new Error(await response.text())
        }

        settings.value = await response.json()
        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
        configStore.showRestartModal = true
        indexStore.msgAlert('success', t('config.updateGlobalSuccess'), 2)
    } catch {
        indexStore.msgAlert('error', t('config.updateGlobalFailed'), 3)
    }
}

async function restart(res: boolean) {
    if (res) {
        await Promise.all(
            configStore.channels.map(({ id }) =>
                fetch(`/api/control/${id}/process`, {
                    method: 'POST',
                    headers: { ...configStore.contentType, ...authStore.authHeader },
                    body: JSON.stringify({ command: 'restart' }),
                }),
            ),
        )
    }

    configStore.showRestartModal = false
}
</script>

<template>
    <div v-if="authStore.role === 'global_admin'" class="w-full max-w-200">
        <h2 class="pt-3 text-3xl">{{ t('config.global') }}</h2>
        <form v-if="!loading" class="mt-5 flex flex-col gap-1" @submit.prevent="save">
            <h3 class="text-xl">{{ t('config.smtp') }}</h3>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpServer') }}</legend>
                <input v-model="settings.smtp_server" type="text" class="input w-full" />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpUser') }}</legend>
                <input v-model="settings.smtp_user" type="text" class="input w-full" />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpPassword') }}</legend>
                <input
                    v-model="smtpPassword"
                    type="password"
                    class="input w-full"
                    :placeholder="settings.smtp_password_set ? t('config.passwordConfigured') : t('config.placeholderPass')"
                />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpPort') }}</legend>
                <input v-model.number="settings.smtp_port" type="number" min="1" max="65535" class="input w-full" />
            </fieldset>
            <fieldset class="fieldset mt-2">
                <label class="fieldset-label text-base-content">
                    <input v-model="settings.smtp_starttls" type="checkbox" class="checkbox" />
                    {{ t('config.smtpStarttls') }}
                </label>
            </fieldset>

            <div class="my-5 flex gap-1">
                <button type="submit" class="btn" :class="isChanged() ? 'btn-error' : 'btn-primary'" :disabled="!isChanged()">
                    {{ t('config.save') }}
                </button>
                <button v-if="isChanged()" type="button" class="btn btn-primary text-xl" @click="getSettings">
                    <i class="bi-arrow-repeat" />
                </button>
            </div>
        </form>
    </div>
    <GenericModal
        :title="t('config.restartTile')"
        :text="t('config.restartText')"
        :show="configStore.showRestartModal"
        :modal-action="restart"
    />
</template>
