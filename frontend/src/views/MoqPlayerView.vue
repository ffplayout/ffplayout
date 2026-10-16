<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import MoqPlayer from '@/components/utils/MoqPlayer.vue'
const route = useRoute()
const { t } = useI18n()
const channel = computed(() => Number(route.query.channel))
const validChannel = computed(() => Number.isSafeInteger(channel.value) && channel.value > 0)
</script>
<template>
    <div class="min-h-dvh flex items-center justify-center p-4">
        <div class="w-full max-w-5xl">
            <MoqPlayer v-if="validChannel" :key="channel" :channel="channel" public-access />
            <p v-else role="status">{{ t('config.moqPublicUnavailable') }}</p>
        </div>
    </div>
</template>
