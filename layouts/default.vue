<template>
    <div class="min-h-screen bg-base-200">
        <div v-if="authStore.isLogin && route.name !== 'index'">
            <Menu />
        </div>

        <main class="h-[calc(100%-52px)]">
            <slot />
        </main>

        <Alert />
    </div>
</template>

<script setup lang="ts">
const configStore = useConfig()
const authStore = useAuth()

const route = useRoute()

console.log(route.name)

onMounted(() => {
    // @ts-ignore
    // new $bootstrap.Tooltip(document.body, {
    //     selector: "[data-tooltip=tooltip]",
    //     container: "body"
    // })
})
await configStore.nuxtClientInit()
</script>
