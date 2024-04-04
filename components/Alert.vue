<template>
        <div
            v-if="indexStore.showAlert"
            class="alert show alert-dismissible fade media-alert position-fixed"
            :class="indexStore.alertVariant"
            role="alert"
        >
            {{ indexStore.alertMsg }}
            <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close" @click="indexStore.showAlert=false"></button>
        </div>
</template>

<script setup lang="ts">
const indexStore = useIndex()
</script>
