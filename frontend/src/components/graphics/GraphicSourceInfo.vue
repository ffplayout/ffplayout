<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import type { GraphicMetadata } from '@/composables/graphicMetadata'
defineProps<{ info?: GraphicMetadata }>()
const { t } = useI18n()
</script>
<template>
    <span v-if="info" class="text-xs text-base-content/70 tabular-nums" role="status">
        <span v-if="info.width && info.height"
            >{{ t('graphicsEditor.sourceResolution') }}: {{ info.width }} × {{ info.height }} px</span
        >
        <span v-if="info.source_duration != null"
            >{{ info.width && info.height ? ' · ' : '' }}{{ t('graphicsEditor.sourceDuration') }}:
            {{ info.source_duration.toLocaleString(undefined, { maximumFractionDigits: 3 }) }} s</span
        >
    </span>
</template>
