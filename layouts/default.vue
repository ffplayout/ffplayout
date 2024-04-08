<template>
    <div class="min-h-screen bg-base-200">
        <div v-if="authStore.isLogin && route.name !== 'index'">
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

onMounted(() => {
    // @ts-ignore
    // new $bootstrap.Tooltip(document.body, {
    //     selector: "[data-tooltip=tooltip]",
    //     container: "body"
    // })
})
await configStore.nuxtClientInit()
</script>
