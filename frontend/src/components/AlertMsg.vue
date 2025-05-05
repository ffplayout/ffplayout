<template>
    <div
        v-if="indexStore.showAlert"
        role="alert"
        class="alert fixed top-20 right-3 z-40 w-auto min-w-64 max-w-[90%] md:max-w-[50%] xl:max-w-[40%] text-black!"
        :class="`alert-${indexStore.alertVariant}`"
    >
        <SvgIcon :name="indexStore.alertVariant" />
        {{ indexStore.alertMsg }}
    </div>
</template>

<script setup lang="ts">
import { useIndex } from '@/stores/index'

import SvgIcon from '@/components/SvgIcon.vue'

const indexStore = useIndex()
</script>
