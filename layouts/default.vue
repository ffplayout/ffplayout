<template>
    <div class="min-h-screen bg-base-200">
        <div v-if="authStore.isLogin && !String(route.name).includes('index')" class="sticky top-0 z-10">
            <HeaderMenu />
        </div>

        <main :class="authStore.isLogin && !String(route.name).includes('index') ? 'h-[calc(100%-52px)]' : 'h-full'">
            <slot />
        </main>

        <AlertMsg />
    </div>
</template>

<script setup lang="ts">
const colorMode = useColorMode()
const configStore = useConfig()
const authStore = useAuth()
const indexStore = useIndex()

const route = useRoute()

await configStore.nuxtClientInit()

if (colorMode.value === 'dark') {
    indexStore.darkMode = true
} else {
    indexStore.darkMode = false
}
</script>
