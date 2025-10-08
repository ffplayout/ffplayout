<template>
    <div class="min-h-screen bg-base-200">
        <div v-if="authStore.isLogin && !String(route.name).includes('home')" class="sticky top-0 z-10">
            <HeaderMenu />
        </div>

        <main :class="authStore.isLogin && !String(route.name).includes('home') ? 'h-[calc(100%-52px)]' : 'h-full'">
            <RouterView />
        </main>

        <AlertMsg />
    </div>
</template>
<script setup lang="ts">
import { computed, ref, onBeforeMount } from 'vue'
import { RouterView, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'

import AlertMsg from '@/components/AlertMsg.vue'
import HeaderMenu from '@/components/HeaderMenu.vue'

const { locale } = useI18n()
const authStore = useAuth()
const indexStore = useIndex()
const route = useRoute()

const language = localStorage.getItem('language')

locale.value = language || 'en'

const darkThemeMq = window.matchMedia('(prefers-color-scheme: dark)')
const theme = ref(localStorage.getItem('theme'))

const preferDark = computed(() => {
    return theme.value === 'dark' || (!theme.value && darkThemeMq.matches)
})

onBeforeMount(() => {
    indexStore.darkMode = preferDark.value

    darkThemeMq.addEventListener('change', (e) => {
        indexStore.darkMode = e.matches
    })

    window.addEventListener('storage', (e) => {
        if (e.key === 'theme') {
            theme.value = e.newValue
            indexStore.darkMode = preferDark.value
        }
    })
})

useHead({
    htmlAttrs: {
        lang: computed(() => locale.value),
        'data-theme': computed(() => (indexStore.darkMode ? 'dark' : 'light')),
    },
})
</script>
