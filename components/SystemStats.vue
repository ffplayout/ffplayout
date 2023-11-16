<template>
    <div class="row sys-container text-start">
        <div class="col-4 h-100 bg-warning">
            {{ sysStat.system.name }} {{ sysStat.system.version }} {{ sysStat.system.kernel }}
        </div>
        <div class="col-5">CPU Cores: {{ sysStat.cpu.cores }} Used {{ sysStat.cpu.usage }}</div>
        <div class="col-5">Load: {{ sysStat.load.one }} | {{ sysStat.load.five }} | {{ sysStat.load.fifteen }}</div>
        <div class="col-5">Memory Total: {{ fileSize(sysStat.memory.total) }} Used: {{ fileSize(sysStat.memory.used) }}</div>
        <div class="col-5">Network Name: {{ sysStat.network?.name }} In: {{ fileSize(sysStat.network?.current_in) }}</div>
        <div class="col-5">Storage Path: {{ sysStat.storage?.path }} Size: {{ fileSize(sysStat.storage?.total) }} Used: {{fileSize(sysStat.storage?.used)}}</div>
        <div class="col-5">Swap Total: {{ fileSize(sysStat.swap.total) }} Used: {{ fileSize(sysStat.swap.used) }}</div>
    </div>
</template>
<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'

const { fileSize } = stringFormatter()

const authStore = useAuth()
const configStore = useConfig()
const contentType = { 'content-type': 'application/json,charset=UTF-8' }
const timer = ref()
const sysStat = ref({
        cpu: { cores: 0.0, usage: 0.0 },
        load: { one: 0.0, five: 0.0, fifteen: 0.0 },
        memory: { total: 0.0, used: 0.0, free: 0.0 },
        network: { name: "", current_in: 0.0, current_out: 0.0, total_in: 0.0, total_out: 0.0 },
        storage: { path: "", total: 0.0, used: 0.0 },
        swap: { total: 0.0, used: 0.0, free: 0.0 },
        system: { name: "", kernel: "", version: "" },
    } as SystemStatistics)

onMounted(() => {
    status()
})

onBeforeUnmount(() => {
    if (timer.value) {
        clearTimeout(timer.value)
    }
})

async function status() {
    systemStatus()

    async function setStatus(resolve: any) {
        /*
            recursive function as a endless loop
        */
        systemStatus()

        timer.value = setTimeout(() => setStatus(resolve), 1000)
    }
    return new Promise((resolve) => setStatus(resolve))
}

async function systemStatus() {
    const channel = configStore.configGui[configStore.configID].id

    await $fetch(`/api/system/${channel}`, {
        method: 'GET',
        headers: { ...contentType, ...authStore.authHeader },
    }).then((stat: SystemStatistics) => {
        console.log(stat)
        sysStat.value = stat
    })
}
</script>
<style>
.sys-container {
    min-height: 500px,
}
</style>
