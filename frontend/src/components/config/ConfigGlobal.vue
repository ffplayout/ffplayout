<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { cloneDeep } from 'es-toolkit/object'
import { isEqual } from 'es-toolkit/predicate'
import { useI18n } from 'vue-i18n'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const settings = ref({} as GlobalSettings)
const savedSettings = ref({} as GlobalSettings)
const smtpPassword = ref('')
const notificationToken = ref('')
const loading = ref(true)

onMounted(getSettings)

async function getSettings() {
    loading.value = true

    try {
        settings.value = await authFetch<GlobalSettings>('/api/global', {
            headers: authStore.authHeader,
        })
        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
        notificationToken.value = ''
    } catch {
        indexStore.msgAlert('error', t('config.updateGlobalFailed'), 3)
    } finally {
        loading.value = false
    }
}

function isChanged() {
    return (
        smtpPassword.value.length > 0 ||
        notificationToken.value.length > 0 ||
        !isEqual(settings.value, savedSettings.value)
    )
}

async function save() {
    try {
        settings.value = await authFetch<GlobalSettings>('/api/global', {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({
                smtp_server: settings.value.smtp_server,
                smtp_user: settings.value.smtp_user,
                smtp_password: smtpPassword.value,
                smtp_starttls: settings.value.smtp_starttls,
                smtp_port: settings.value.smtp_port,
                notification_server: settings.value.notification_server,
                notification_token: notificationToken.value,
            }),
        })

        savedSettings.value = cloneDeep(settings.value)
        smtpPassword.value = ''
        notificationToken.value = ''
        await configStore.getPlayoutConfig()
        indexStore.msgAlert('success', t('config.updateGlobalSuccess'), 2)
    } catch {
        indexStore.msgAlert('error', t('config.updateGlobalFailed'), 3)
    }
}
</script>

<template>
    <div v-if="authStore.role === 'global_admin'" class="w-full max-w-200">
        <h2 class="pt-3 text-3xl">{{ t('config.global') }}</h2>
        <form v-if="!loading" class="mt-5 flex flex-col gap-1" @submit.prevent="save">
            <h3 class="text-xl">{{ t('config.smtp') }}</h3>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpServer') }}</legend>
                <input v-model="settings.smtp_server" type="text" class="input w-full" name="smtp_server" />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpUser') }}</legend>
                <input v-model="settings.smtp_user" type="text" class="input w-full" name="smtp_user" />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.smtpPassword') }}</legend>
                <input
                    v-model="smtpPassword"
                    type="password"
                    class="input w-full"
                    :placeholder="
                        settings.smtp_password_set ? t('config.passwordConfigured') : t('config.placeholderPass')
                    "
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

            <h3 class="mt-6 text-xl">{{ t('config.notification') }}</h3>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.notificationServer') }}</legend>
                <input
                    v-model="settings.notification_server"
                    type="url"
                    name="notification_server"
                    placeholder="https://push.example.org"
                    class="input w-full"
                />
            </fieldset>
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('config.notificationToken') }}</legend>
                <input
                    v-model="notificationToken"
                    type="password"
                    class="input w-full"
                    :placeholder="
                        settings.notification_token_set ? t('config.tokenConfigured') : t('config.placeholderToken')
                    "
                />
            </fieldset>

            <div class="my-5 flex gap-1">
                <button
                    type="submit"
                    class="btn"
                    :class="isChanged() ? 'btn-error' : 'btn-primary'"
                    :disabled="!isChanged()"
                >
                    {{ t('config.save') }}
                </button>
                <button v-if="isChanged()" type="button" class="btn btn-primary text-xl" @click="getSettings">
                    <i class="bi-arrow-repeat" />
                </button>
            </div>
        </form>
    </div>
</template>
