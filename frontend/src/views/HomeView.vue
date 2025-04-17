<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div v-if="authStore.isLogin" class="flex flex-col justify-center items-center w-full p-5">
            <SystemStats v-if="configStore.channels.length > 0" />

            <div class="w-full flex flex-wrap justify-center gap-1 md:gap-0 md:join mt-5">
                <RouterLink to="/player" class="btn btn-primary join-item px-2">
                    {{ t('button.player') }}
                </RouterLink>
                <RouterLink to="/media" class="btn btn-primary join-item px-2">
                    {{ t('button.media') }}
                </RouterLink>
                <RouterLink
                    v-if="configStore.playout?.text?.add_text && !configStore.playout?.text?.text_from_filename"
                    to="/message"
                    class="btn btn-primary join-item px-2"
                >
                    {{ t('button.message') }}
                </RouterLink>
                <RouterLink to="/logging" class="btn btn-primary join-item px-2">
                    {{ t('button.logging') }}
                </RouterLink>
                <div class="dropdown">
                    <div tabindex="0" role="button" class="btn btn-primary bg-primary/50 px-2 join-item">
                        {{ selectedLang?.name }}
                    </div>
                    <ul tabindex="0" class="dropdown-content menu bg-base-100 rounded-box z-1 w-52 p-2 shadow-sm">
                        <li v-for="lang in locales" :key="lang.code" :title="lang.name">
                            <button class="px-1 py-2 rounded" @click="changeLang(lang)">
                                {{ lang.name }}
                            </button>
                        </li>
                    </ul>
                </div>
                <RouterLink to="/configure" class="btn btn-primary join-item px-2" :title="t('button.configure')">
                    <i class="bi bi-gear text-[17px]" />
                </RouterLink>
                <label class="join-item btn btn-primary swap swap-rotate px-2">
                    <input
                        type="checkbox"
                        :checked="indexStore.darkMode"
                        class="focus-within:!outline-0"
                        @change="toggleTheme"
                    />
                    <i class="swap-on bi bi-brightness-high text-[18px]"></i>
                    <i class="swap-off bi bi-moon text-[18px]"></i>
                </label>
                <button class="btn btn-primary join-item px-2" @click="logout()" :title="t('button.logout')">
                    <i class="bi bi-door-closed text-[18px]" />
                </button>
            </div>
        </div>
        <div v-else class="w-96 min-w-full flex flex-col justify-center items-center px-4">
            <h1 class="text-6xl xs:text-8xl">ffplayout</h1>

            <form class="mt-10" @submit.prevent="login">
                <input
                    v-model="formUsername"
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
                        <button type="submit" class="btn btn-primary">
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

<script setup lang="ts">
import { computed, ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import { locales } from '@/i18n'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

import SvgIcon from '@/components/SvgIcon.vue'
import SystemStats from '@/components/SystemStats.vue'

const { locale, t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const selectedLang = ref()
const formError = ref('')
const showLoginError = ref(false)
const formUsername = ref('')
const formPassword = ref('')

onMounted(() => {
    selectedLang.value = locales.find((loc: any) => loc.code === locale.value)
})

useHead({
    title: computed(() => (authStore.isLogin ? 'System' : 'Login')),
})

async function login() {
    try {
        const status = await authStore.obtainToken(formUsername.value, formPassword.value)

        formUsername.value = ''
        formPassword.value = ''
        formError.value = ''

        if (status === 401 || status === 400 || status === 403) {
            formError.value = t('alert.wrongLogin')
            showLoginError.value = true

            setTimeout(() => {
                showLoginError.value = false
            }, 3000)
        }

        await configStore.configInit()
    } catch (e) {
        formError.value = e as string
    }
}

function toggleTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        localStorage.setItem('theme', 'dark')
        // document.documentElement.setAttribute('data-theme', 'dark')
    } else {
        localStorage.setItem('theme', 'light')
        // document.documentElement.setAttribute('data-theme', 'light')
    }
}

async function logout() {
    try {
        authStore.removeToken()
    } catch (e) {
        formError.value = e as string
    }
}

async function changeLang(lang: any) {
    selectedLang.value = lang
    locale.value = lang.code
    localStorage.setItem('language', lang.code)
}
</script>
