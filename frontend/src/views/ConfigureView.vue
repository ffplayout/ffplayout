<template>
    <div class="flex flex-wrap xs:flex-nowrap w-full xs:h-[calc(100vh-60px)] xs:max-h-[calc(100vh-60px)] ps-1">
        <div class="xs:flex-none w-full xs:w-[68px] join join-horizontal xs:join-vertical me-1 pt-7">
            <button
                class="join-item btn btn-sm btn-primary duration-500"
                :class="activeConf === 1 && 'bg-base-100/40'"
                @click="activeConf = 1"
            >
                {{ t('config.channel') }}
            </button>
            <button
                v-if="authStore.role === 'global_admin'"
                class="join-item btn btn-sm btn-primary duration-500"
                :class="activeConf === 2 && 'bg-base-100/40'"
                @click="activeConf = 2"
            >
                Advanced
            </button>
            <button
                v-if="authStore.role !== 'user'"
                class="join-item btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 3 && 'bg-base-100/40'"
                @click="activeConf = 3"
            >
                Playout
            </button>
            <button
                class="join-item btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 4 && 'bg-base-100/40'"
                @click="activeConf = 4"
            >
                {{ t('config.user') }}
            </button>
        </div>
        <div class="grow mt-6 px-3 xs:px-6 overflow-auto">
            <div>
                <div v-if="activeConf === 1" class="w-full flex justify-center">
                    <ConfigChannel />
                </div>

                <div v-if="activeConf === 2" class="w-full flex justify-center">
                    <ConfigAdvanced />
                </div>

                <div v-else-if="activeConf === 3" class="w-full flex justify-center">
                    <ConfigPlayout />
                </div>

                <div v-else-if="activeConf === 4" class="w-full flex justify-center">
                    <ConfigUser />
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import ConfigChannel from '@/components/ConfigChannel.vue'
import ConfigAdvanced from '@/components/ConfigAdvanced.vue'
import ConfigPlayout from '@/components/ConfigPlayout.vue'
import ConfigUser from '@/components/ConfigUser.vue'

import { useAuth } from '@/stores/auth'

const { t } = useI18n()
const authStore = useAuth()

useHead({
    title: computed(() => t('button.configure'))
})

const activeConf = ref(1)
</script>
