<template>
    <div class="row sys-container text-start bg-secondary border border-3 rounded">
        <div class="col-6 bg-primary p-2 border-top border-start border-end fs-2">
            {{ sysStat.system.name }} {{ sysStat.system.version }}
        </div>
        <div class="col-6 p-2 border">
            <div class="fs-4">CPU</div>
            <div class="row">
                <div class="col"><strong>Cores:</strong> {{ sysStat.cpu.cores }}</div>
                <div class="col"><strong>Usage:</strong> {{ sysStat.cpu.usage.toFixed(2) }}%</div>
            </div>
        </div>
        <div v-if="sysStat.system.kernel" class="col-6 bg-primary border-start border-end border-bottom">
            {{ sysStat.system.kernel }}
        </div>
        <div class="col-6 p-2 border">
            <div class="fs-4">Load</div>
            <div class="row">
                <div class="col">{{ sysStat.load.one }}</div>
                <div class="col">{{ sysStat.load.five }}</div>
                <div class="col">{{ sysStat.load.fifteen }}</div>
            </div>
        </div>
        <div class="col-6 border">
            <div class="fs-4">Memory</div>
            <div class="row">
                <div class="col"><strong>Total:</strong> {{ fileSize(sysStat.memory.total) }}</div>
                <div class="col"><strong>Usage:</strong> {{ fileSize(sysStat.memory.used) }}</div>
            </div>
        </div>
        <div class="col-6 p-2 border">
            <div class="fs-4">Swap</div>
            <div class="row">
                <div class="col"><strong>Total:</strong> {{ fileSize(sysStat.swap.total) }}</div>
                <div class="col"><strong>Usage:</strong> {{ fileSize(sysStat.swap.used) }}</div>
            </div>
        </div>
        <div class="col-6 p-2 border">
            <div class="fs-4">
                Network <span v-if="sysStat.network" class="fs-6">{{ sysStat.network?.name }}</span>
            </div>
            <div class="row">
                <div class="col-6"><strong>In:</strong> {{ fileSize(sysStat.network?.current_in) }}</div>
                <div class="col-6"><strong>Out:</strong> {{ fileSize(sysStat.network?.current_out) }}</div>
                <div class="col-6">{{ fileSize(sysStat.network?.total_in) }}</div>
                <div class="col-6">{{ fileSize(sysStat.network?.total_out) }}</div>
            </div>
        </div>
        <div v-if="sysStat.storage?.path" class="col-6 p-2 border">
            <div class="fs-4">Storage</div>
            <div v-if="sysStat.storage"><strong>Device:</strong> {{ sysStat.storage?.path }}</div>

            <div class="row" v-if="sysStat.storage">
                <div class="col"><strong>Size:</strong> {{ fileSize(sysStat.storage?.total) }}</div>
                <div class="col"><strong>Used:</strong> {{ fileSize(sysStat.storage?.used) }}</div>
            </div>
        </div>
        <div v-else class="col-6 bg-primary p-2 border" />
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
    network: { name: '', current_in: 0.0, current_out: 0.0, total_in: 0.0, total_out: 0.0 },
    storage: { path: '', total: 0.0, used: 0.0 },
    swap: { total: 0.0, used: 0.0, free: 0.0 },
    system: { name: '', kernel: '', version: '' },
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

    await $fetch<SystemStatistics>(`/api/system/${channel}`, {
        method: 'GET',
        headers: { ...contentType, ...authStore.authHeader },
    }).then((stat: SystemStatistics) => {
        sysStat.value = stat
    })
}
</script>
<style>
.sys-container {
    max-width: 640px;
    min-height: 300px;
}
</style>
