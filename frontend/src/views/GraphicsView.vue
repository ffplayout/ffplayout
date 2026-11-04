<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { useRoute } from 'vue-router'
const { t } = useI18n()
const route = useRoute()
useHead({ title: computed(() => t('graphicsPage.title')) })
const tabs = ['graphics-live', 'graphics-text', 'graphics-logo'] as const
</script>
<template>
    <section class="w-full px-4 sm:px-8 py-6 overflow-auto">
        <h1 class="text-2xl mb-4">{{ t('graphicsPage.title') }}</h1>
        <nav class="tabs tabs-border mb-6" :aria-label="t('graphicsPage.title')">
            <RouterLink
                v-for="tab in tabs"
                :key="tab"
                :to="{ name: tab, query: route.query }"
                class="tab"
                :class="{ 'tab-active': route.name === tab }"
                :aria-current="route.name === tab ? 'page' : undefined"
                >{{ t(`graphicsPage.tabs.${tab}`) }}</RouterLink
            >
        </nav>
        <RouterView />
    </section>
</template>
