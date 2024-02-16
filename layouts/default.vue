<template>
    <main>
        <slot />
        <div
            v-if="indexStore.showAlert"
            class="alert show alert-dismissible fade media-alert position-fixed"
            :class="indexStore.alertVariant"
            role="alert"
        >
            {{ indexStore.alertMsg }}
            <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close" @click="indexStore.showAlert=false"></button>
        </div>
    </main>
</template>

<script setup lang="ts">
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'

const { $bootstrap } = useNuxtApp()
const configStore = useConfig()
const indexStore = useIndex()

useHead({
    htmlAttrs: {
        lang: 'en',
        "data-bs-theme": "dark"
    }
})

onMounted(() => {
    // @ts-ignore
    new $bootstrap.Tooltip(document.body, {
        selector: "[data-tooltip=tooltip]",
        container: "body"
    })
})
await configStore.nuxtClientInit()
</script>

