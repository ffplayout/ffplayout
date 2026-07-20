<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { useRouter } from 'vue-router'

import { locales } from '@/i18n'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

import SystemStats from '@/components/SystemStats.vue'

const { locale, t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const router = useRouter()

const selectedLang = ref()
const formError = ref('')

onMounted(() => {
    selectedLang.value = locales.find((loc: any) => loc.code === locale.value || loc.language === locale.value)
})

useHead({
    title: 'System',
})

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
        await authStore.logout()
        await router.push({ name: 'login' })
    } catch (e) {
        formError.value = e as string
    }
}

async function changeLang(lang: any) {
    selectedLang.value = lang
    locale.value = lang.code
    localStorage.setItem('language', lang.code)
}

function channelLink(path: string) {
    const channelId = configStore.channels[configStore.i]?.id

    return channelId ? { path, query: { channel: channelId } } : path
}
</script>
<template>
    <div class="w-full min-h-screen xs:h-full flex justify-center items-center">
        <div class="flex flex-col justify-center items-center w-full p-5">
            <SystemStats v-if="configStore.channels.length > 0" />

            <div class="w-full flex flex-wrap justify-center gap-1 md:gap-0 md:join mt-5">
                <RouterLink :to="channelLink('/player')" class="btn btn-primary join-item px-2">
                    {{ t('button.player') }}
                </RouterLink>
                <RouterLink :to="channelLink('/media')" class="btn btn-primary join-item px-2">
                    {{ t('button.media') }}
                </RouterLink>
                <RouterLink :to="channelLink('/message')" class="btn btn-primary join-item px-2">
                    {{ t('button.message') }}
                </RouterLink>
                <RouterLink :to="channelLink('/logging')" class="btn btn-primary join-item px-2">
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
                <RouterLink :to="channelLink('/configure')" class="btn btn-primary join-item px-2" :title="t('button.configure')">
                    <i class="bi bi-gear text-[17px]" />
                </RouterLink>
                <label class="join-item btn btn-primary swap swap-rotate px-2">
                    <input
                        type="checkbox"
                        :checked="indexStore.darkMode"
                        class="focus-within:outline-0!"
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
    </div>
</template>
