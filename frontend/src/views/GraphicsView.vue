<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'
const { t } = useI18n()
const route = useRoute()
const tabs = ['graphics-live', 'graphics-text', 'graphics-logo', 'graphics-settings'] as const
const activeTab = computed(() => tabs.find((tab) => tab === route.name) ?? tabs[0])
const title = computed(() => t(`graphicsPage.tabs.${activeTab.value}`))
useHead({
    title: computed(() =>
        activeTab.value === 'graphics-live' ? title.value : `${t('graphicsPage.title')} · ${title.value}`,
    ),
})
</script>
<template>
    <div class="flex flex-wrap xs:flex-nowrap w-full xs:h-[calc(100vh-60px)] xs:max-h-[calc(100vh-60px)] ps-1">
        <nav
            class="xs:flex-none w-full xs:w-28 join join-horizontal xs:join-vertical me-1 pt-7"
            :aria-label="t('graphicsPage.title')"
        >
            <RouterLink
                v-for="(tab, index) in tabs"
                :key="tab"
                :to="{ name: tab, query: route.query }"
                class="join-item btn btn-sm btn-primary duration-500"
                :class="{ 'bg-base-100/40': route.name === tab, 'mt-1': index > 0 }"
                :aria-current="route.name === tab ? 'page' : undefined"
            >
                {{ t(`graphicsPage.tabs.${tab}`) }}
            </RouterLink>
        </nav>
        <section class="grow min-w-0 mt-6 px-3 xs:px-6 overflow-auto">
            <h1 class="text-2xl mb-4">{{ title }}</h1>
            <RouterView />
        </section>
    </div>
</template>
