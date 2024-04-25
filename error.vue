<template>
    <NuxtLayout>
        <div class="w-full h-full flex justify-center items-center mt-20">
            <div v-if="props.error?.statusCode === 404">
                <h1 class="text-center text-6xl">404</h1>
                <p class="text-center font-bold mt-6">
                    {{ $t('error.notFound') }}
                </p>
            </div>
            <div v-else-if="props.error?.statusCode === 500">
                <h1 class="text-center text-6xl">{{ props.error.statusCode }}</h1>
                <p class="text-center font-bold mt-6">
                    {{ $t('error.serverError') }}
                </p>
            </div>
        </div>
    </NuxtLayout>
</template>

<script setup lang="ts">
import type { NuxtError } from '#app'
const localePath = useLocalePath()

const props = defineProps({
    error: {
        type: Object,
        default: {} as NuxtError,
    },
})

// onMounted(() => {
//     const statusCode = props.error?.statusCode || 400

//     if (statusCode >= 400) {
//         setTimeout(() => {
//             reloadNuxtApp({
//                 path: localePath({ name: 'index' }),
//                 ttl: 1000,
//             })
//         }, 3000)
//     }
// })
</script>
