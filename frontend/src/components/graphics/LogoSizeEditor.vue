<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { formatLogoSize, parseLogoSize, type LogoSizeMode } from '@/composables/logoSize'

const model = defineModel<string>({ required: true })
const { t } = useI18n()
const mode = ref<LogoSizeMode>('original')
const size = computed(() => ({ ...parseLogoSize(model.value), mode: mode.value }))

watch(
    model,
    (value) => {
        const parsed = parseLogoSize(value)
        // Both automatic axes use the same stored value in either unit mode.
        if (value !== '-1:-1' || mode.value !== 'percent') mode.value = parsed.mode
    },
    { immediate: true, flush: 'sync' },
)

function changeMode(event: Event) {
    const selected = (event.target as HTMLSelectElement).value as LogoSizeMode
    if (selected === 'custom') return
    mode.value = selected
    model.value = formatLogoSize({ mode: selected, width: selected === 'percent' ? 10 : '', height: '' })
}

function changeDimension(axis: 'width' | 'height', event: Event) {
    const input = event.target as HTMLInputElement
    if (!input.validity.valid) return
    model.value = formatLogoSize({ ...size.value, [axis]: input.value === '' ? '' : input.valueAsNumber })
}
</script>

<template>
    <fieldset class="fieldset gap-2">
        <legend class="fieldset-legend">{{ t('graphicsPage.logoScale') }}</legend>
        <label class="fieldset">
            <span>{{ t('graphicsPage.logoSizeMode') }}</span>
            <select :value="size.mode" class="select select-sm w-full" @change="changeMode">
                <option value="original">{{ t('graphicsPage.logoOriginalSize') }}</option>
                <option value="pixels">{{ t('graphicsPage.logoPixels') }}</option>
                <option value="percent">{{ t('graphicsPage.logoPercent') }}</option>
                <option v-if="size.mode === 'custom'" value="custom">{{ t('graphicsPage.logoCustomSize') }}</option>
            </select>
        </label>
        <div v-if="size.mode === 'pixels' || size.mode === 'percent'" class="grid grid-cols-2 gap-4">
            <label v-for="axis in ['width', 'height'] as const" :key="axis" class="fieldset">
                <span>{{ t(`config.${axis}`) }} ({{ size.mode === 'percent' ? '%' : 'px' }})</span>
                <input
                    :value="size[axis]"
                    type="number"
                    :min="size.mode === 'percent' ? 0.01 : 2"
                    :step="size.mode === 'percent' ? 0.01 : 1"
                    :placeholder="t('graphicsPage.logoAutomatic')"
                    class="input input-sm w-full"
                    @input="changeDimension(axis, $event)"
                />
            </label>
        </div>
        <p v-if="size.mode === 'pixels' || size.mode === 'percent'" class="text-xs text-base-content/70">
            {{ t('graphicsPage.logoSizeHelp') }}
            {{ size.mode === 'percent' ? t('graphicsPage.logoPercentHelp') : '' }}
        </p>
        <details :open="size.mode === 'custom'">
            <summary class="cursor-pointer text-xs">{{ t('graphicsPage.logoCustomSize') }}</summary>
            <label class="fieldset">
                <span>{{ t('graphicsPage.logoScale') }}</span>
                <input v-model="model" class="input input-sm w-full" />
                <span class="text-xs">{{ t('config.processingLogoScale') }}</span>
            </label>
        </details>
    </fieldset>
</template>
