<template>
    <main>
        <NuxtPage />
        <div
            v-if="indexStore.showAlert"
            class="alert show alert-dismissible fade media-alert"
            :class="indexStore.alertVariant"
            role="alert"
        >
            {{ indexStore.alertMsg }}
            <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close"></button>
        </div>
    </main>
</template>

<script setup lang="ts">
import { useConfig } from '~/stores/config'
import { useIndex } from '~/stores/index'

const configStore = useConfig()
const indexStore = useIndex()

useHead({
    htmlAttrs: {
        lang: 'en',
        "data-bs-theme": "dark"
    }
})

await configStore.nuxtClientInit()
</script>

