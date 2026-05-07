<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { useRouter } from 'vue-router'

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

import SvgIcon from '@/components/utils/SvgIcon.vue'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const router = useRouter()

const formError = ref('')
const showLoginError = ref(false)
const formPassword = ref('')
const disabled = ref(false)

onMounted(async () => {
    if (authStore.isLogin) {
        await router.push({ name: 'home' })
    }
})

useHead({
    title: 'Login',
})

async function login() {
    disabled.value = true

    try {
        const status = await authStore.obtainVerificationCode(formPassword.value)

        formPassword.value = ''
        formError.value = ''

        if (status === 401 || status === 400 || status === 403) {
            formError.value = t('alert.wrongLogin')
            showLoginError.value = true
            disabled.value = false

            setTimeout(() => {
                showLoginError.value = false
            }, 3000)

            return
        }

        if (status === 200 && authStore.jwtToken.length < 10) {
            indexStore.msgAlert('success', t('alert.verificationSent'))
            await router.push({ name: 'verification' })
            disabled.value = false
            return
        }

        await configStore.configInit()
        await router.push({ name: 'home' })
        disabled.value = false
    } catch (e) {
        disabled.value = false
        formError.value = e as string
    }
}
</script>
<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div class="w-96 min-w-full flex flex-col justify-center items-center px-4">
            <h1 class="text-6xl xs:text-8xl">ffplayout</h1>

            <form class="mt-10" @submit.prevent="login">
                <input
                    v-model="authStore.username"
                    type="text"
                    name="username"
                    :placeholder="t('input.username')"
                    class="input w-full focus:border-base-content/30 focus:outline-base-content/30"
                    required
                />

                <input
                    v-model="formPassword"
                    type="password"
                    name="password"
                    :placeholder="t('input.password')"
                    class="input w-full mt-5 focus:border-base-content/30 focus:outline-base-content/30"
                    required
                />

                <div class="w-full mt-4 grid grid-flow-row-dense grid-cols-12 grid-rows-1 gap-2">
                    <div class="col-span-3">
                        <button type="submit" class="btn btn-primary" :disabled="disabled">
                            {{ t('button.login') }}
                        </button>
                    </div>
                    <div class="col-span-12 sm:col-span-9">
                        <div
                            v-if="showLoginError"
                            role="alert"
                            class="alert alert-error w-auto rounded-sm z-2 h-12 p-[0.7rem]"
                        >
                            <SvgIcon name="error" />
                            <span>{{ formError }}</span>
                        </div>
                    </div>
                </div>
            </form>
        </div>
    </div>
</template>
