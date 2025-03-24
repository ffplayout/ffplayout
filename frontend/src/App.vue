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
import { RouterView, useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'

import AlertMsg from '@/components/AlertMsg.vue'
import HeaderMenu from '@/components/HeaderMenu.vue'

const { locale } = useI18n()
const authStore = useAuth()
const indexStore = useIndex()
const route = useRoute()

const language = localStorage.getItem('language')
const theme = localStorage.getItem('theme')

locale.value = language || 'en-US'
document.documentElement.setAttribute('data-theme', theme || 'dark')
indexStore.darkMode = !theme || theme === 'dark'
</script>
