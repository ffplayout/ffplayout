<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useHead } from '@unhead/vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'

const { t } = useI18n()
const router = useRouter()

const loading = ref(true)
const submitting = ref(false)
const error = ref('')
const confirmPassword = ref('')
const form = ref({
    logs: '',
    playlists: '',
    public: '',
    storage: '',
    shared: false,
    smtp_server: '',
    smtp_user: '',
    smtp_password: '',
    smtp_starttls: false,
    smtp_port: 465,
    username: '',
    mail: '',
    password: '',
    two_factor: true,
})

useHead({ title: t('setup.title') })

onMounted(loadSetup)

async function loadSetup() {
    try {
        const response = await fetch('/api/setup')
        const data = await response.json()

        if (!response.ok || !data.required) {
            await router.replace({ name: 'login' })
            return
        }

        Object.assign(form.value, data.settings)
    } catch {
        error.value = t('setup.loadFailed')
    } finally {
        loading.value = false
    }
}

async function submit() {
    if (form.value.password !== confirmPassword.value) {
        error.value = t('setup.passwordMismatch')
        return
    }

    submitting.value = true
    error.value = ''

    try {
        const response = await fetch('/api/setup', {
            method: 'POST',
            headers: { 'content-type': 'application/json;charset=UTF-8' },
            body: JSON.stringify(form.value),
        })

        if (!response.ok) {
            throw new Error(await response.text())
        }

        await router.replace({ name: 'login' })
    } catch (cause) {
        error.value = cause instanceof Error ? cause.message : t('setup.saveFailed')
    } finally {
        submitting.value = false
    }
}
</script>

<template>
    <div class="w-full min-h-screen flex justify-center py-10 px-4">
        <div class="w-full max-w-200">
            <h1 class="text-4xl">ffplayout</h1>
            <h2 class="mt-2 text-2xl">{{ t('setup.title') }}</h2>
            <form v-if="!loading" class="mt-8 flex flex-col gap-1" @submit.prevent="submit">
                <h3 class="text-xl">{{ t('config.global') }}</h3>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.logsPath') }}</legend>
                    <input v-model="form.logs" name="logs" type="text" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.playlistPath') }}</legend>
                    <input v-model="form.playlists" name="playlists" type="text" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.publicPath') }}</legend>
                    <input v-model="form.public" name="public" type="text" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.storagePath') }}</legend>
                    <input v-model="form.storage" name="storage" type="text" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset mt-2">
                    <label class="fieldset-label text-base-content">
                        <input v-model="form.shared" type="checkbox" class="checkbox" />
                        {{ t('config.sharedStorage') }}
                    </label>
                </fieldset>

                <h3 class="mt-6 text-xl">{{ t('config.smtp') }}</h3>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.smtpServer') }}</legend>
                    <input v-model="form.smtp_server" name="smtp_server" type="text" class="input w-full" />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.smtpUser') }}</legend>
                    <input v-model="form.smtp_user" name="smtp_user" type="text" class="input w-full" />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.smtpPassword') }}</legend>
                    <input v-model="form.smtp_password" type="password" class="input w-full" />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('config.smtpPort') }}</legend>
                    <input v-model.number="form.smtp_port" type="number" min="1" max="65535" class="input w-full" />
                </fieldset>
                <fieldset class="fieldset mt-2">
                    <label class="fieldset-label text-base-content">
                        <input v-model="form.smtp_starttls" type="checkbox" class="checkbox" />
                        {{ t('config.smtpStarttls') }}
                    </label>
                </fieldset>

                <h3 class="mt-6 text-xl">{{ t('setup.admin') }}</h3>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('input.username') }}</legend>
                    <input v-model="form.username" name="username" type="text" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('user.mail') }}</legend>
                    <input v-model="form.mail" name="mail" type="email" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('input.password') }}</legend>
                    <input v-model="form.password" type="password" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset">
                    <legend class="fieldset-legend">{{ t('setup.confirmPassword') }}</legend>
                    <input v-model="confirmPassword" type="password" class="input w-full" required />
                </fieldset>
                <fieldset class="fieldset mt-2">
                    <label class="fieldset-label text-base-content">
                        <input v-model="form.two_factor" type="checkbox" class="checkbox" />
                        {{ t('user.twoFactor') }}
                    </label>
                </fieldset>

                <p v-if="error" class="mt-3 text-error">{{ error }}</p>
                <button type="submit" class="btn btn-primary mt-5 self-start" :disabled="submitting">
                    {{ t('setup.complete') }}
                </button>
            </form>
        </div>
    </div>
</template>
