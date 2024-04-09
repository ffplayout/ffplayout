<template>
    <div class="min-h-screen bg-base-200">
        <div v-if="authStore.isLogin && route.name !== 'index'" class="sticky top-0 z-10">
            <Menu />
        </div>

        <main :class="authStore.isLogin && route.name !== 'index' ? 'h-[calc(100%-52px)]': 'h-full'">
            <slot />
        </main>

        <Alert />
    </div>
</template>

<script setup lang="ts">
const configStore = useConfig()
const authStore = useAuth()

const route = useRoute()

await configStore.nuxtClientInit()
</script>
