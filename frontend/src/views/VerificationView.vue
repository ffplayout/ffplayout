<script setup lang="ts">
import { ref, onBeforeMount } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useAuth } from '@/stores/auth'

const { t } = useI18n()
const router = useRouter()
const auth = useAuth()

const verificationCode = ref('')
const formError = ref('')
const showLoginError = ref(false)

onBeforeMount(async () => {
    if (auth.isLogin) {
        await router.push({ name: 'home' })
    }
})

async function verify() {
    try {
        const status = await auth.verifyCode(verificationCode.value)

        verificationCode.value = ''
        formError.value = ''

        if (status === 401 || status === 400 || status === 403) {
            formError.value = t('verification.wrongCode')
            showLoginError.value = true

            setTimeout(() => {
                showLoginError.value = false
            }, 3000)
        }

        if (status === 200) {
            await auth.selectAuthUser()
            await router.push({ name: 'home' })
        }
    } catch (e) {
        formError.value = e as string

        showLoginError.value = true

        setTimeout(() => {
            showLoginError.value = false
        }, 3000)
    }
}
</script>
<template>
    <div v-if="!auth.isLogin" class="relative w-full min-h-screen xs:h-full flex justify-center items-center">
        <RouterLink :to="{ name: 'login' }" class="btn btn-ghost absolute top-5 left-5"> Login </RouterLink>
        <div class="w-full h-full flex justify-center items-center">
            <div class="w-96 min-w-full flex flex-col justify-center items-center px-4">
                <h1 class="text-6xl xs:text-8xl">ffplayout</h1>

                <form class="mt-10" @submit.prevent="verify">
                    <input
                        v-model="auth.username"
                        type="text"
                        name="username"
                        :placeholder="`${$t('user.name')} / ${$t('user.mail')}`"
                        class="input w-full focus:border-base-content/30 focus:outline-base-content/30"
                        required
                    />

                    <input
                        v-model="verificationCode"
                        type="text"
                        :placeholder="$t('verification.codePlaceholder')"
                        class="input w-full mt-5 focus:border-base-content/30 focus:outline-base-content/30"
                        required
                    />

                    <div class="w-full mt-4 grid grid-flow-row-dense grid-cols-12 grid-rows-1 gap-2">
                        <div class="col-span-3">
                            <button type="submit" class="btn btn-primary">
                                {{ $t('verification.verify') }}
                            </button>
                        </div>
                        <div class="col-span-12 sm:col-span-9">
                            <div
                                v-if="showLoginError"
                                role="alert"
                                class="alert alert-error w-auto rounded-sm z-2 h-12 p-[0.7rem]"
                            >
                                <i class="bi bi-exclamation-triangle-fill"></i>
                                <span>{{ formError }}</span>
                            </div>
                        </div>
                    </div>
                </form>
            </div>
        </div>
    </div>
</template>
