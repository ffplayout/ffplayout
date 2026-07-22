<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import { RouterLink, RouterView, useRoute } from 'vue-router'

import { useAuth } from '@/stores/auth'

const { t } = useI18n()
const authStore = useAuth()
const route = useRoute()

useHead({
    title: computed(() => t('button.configure'))
})

const isChannelRoute = computed(() => route.name === 'configure-channel')
const isPlayoutRoute = computed(() => route.name === 'configure-playout')
const isUserRoute = computed(() => route.name === 'configure-user')
const isGlobalRoute = computed(() => route.name === 'configure-global')
const channelQuery = computed(() => ({ channel: route.query.channel }))
</script>
<template>
    <div class="flex flex-wrap xs:flex-nowrap w-full xs:h-[calc(100vh-60px)] xs:max-h-[calc(100vh-60px)] ps-1">
        <div class="xs:flex-none w-full xs:w-17 join join-horizontal xs:join-vertical me-1 pt-7">
            <RouterLink
                :to="{ name: 'configure-channel', query: channelQuery }"
                class="join-item btn btn-sm btn-primary duration-500"
                :class="isChannelRoute && 'bg-base-100/40'"
            >
                {{ t('config.channel') }}
            </RouterLink>
            <RouterLink
                v-if="authStore.role !== 'user'"
                :to="{ name: 'configure-playout', query: channelQuery }"
                class="join-item btn btn-sm btn-primary mt-1 duration-500"
                :class="isPlayoutRoute && 'bg-base-100/40'"
            >
                Playout
            </RouterLink>
            <RouterLink
                :to="{ name: 'configure-user', query: channelQuery }"
                class="join-item btn btn-sm btn-primary mt-1 duration-500"
                :class="isUserRoute && 'bg-base-100/40'"
            >
                {{ t('config.user') }}
            </RouterLink>
            <RouterLink
                v-if="authStore.role === 'global_admin'"
                :to="{ name: 'configure-global', query: channelQuery }"
                class="join-item btn btn-sm btn-primary mt-1 duration-500"
                :class="isGlobalRoute && 'bg-base-100/40'"
            >
                {{ t('config.global') }}
            </RouterLink>
        </div>
        <div class="grow mt-6 px-3 xs:px-6 overflow-auto">
            <div>
                <div class="w-full flex justify-center">
                    <RouterView />
                </div>
            </div>
        </div>
    </div>
</template>
