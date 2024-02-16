<template>
    <NuxtLayout>
        <div class="container d-flex align-items-center justify-content-center">
            <div v-if="props.error?.statusCode === 404">
                <h1 class="display-1 text-center">404</h1>
                <p class="text-center mt-10">Page not found</p>
            </div>
            <div v-else-if="props.error?.statusCode === 500">
                <h1 class="display-1 text-center">{{ props.error.statusCode }}</h1>
                <p class="text-center mt-10">Internal server error</p>
            </div>
        </div>
    </NuxtLayout>
</template>

<script setup lang="ts">
import type { NuxtError } from '#app'

const props = defineProps({
    error: Object as () => NuxtError,
})

onMounted(() => {
    const statusCode = props.error?.statusCode || 400

    if (statusCode >= 400) {
        setTimeout(() => {
            reloadNuxtApp({
                path: '/',
                ttl: 1000,
            })
        }, 3000)
    }
})
</script>
