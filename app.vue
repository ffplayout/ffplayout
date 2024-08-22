<template>
    <div>
        <NuxtLayout>
            <NuxtPage />
        </NuxtLayout>
    </div>
</template>
<script setup lang="ts">
const configStore = useConfig()

await useAsyncData('init', () => configStore.configInit())
</script>
