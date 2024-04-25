<template>
    <div class="flex items-center ps-4 pe-1">
        <div
            class="w-2 h-2 rounded-full"
            :class="indexStore.sseConnected ? 'bg-success shadow shadow-success' : 'bg-error shadow shadow-error'"
            :title="indexStore.sseConnected ? $t('socketConnected') : $t('socketDisconnected')"
        />
    </div>
</template>
<script setup lang="ts">
const indexStore = useIndex()
</script>
