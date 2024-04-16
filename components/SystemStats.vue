<template>
    <div class="grid grid-cols-1 xs:grid-cols-2 border-4 rounded-md border-primary text-left shadow max-w-[756px]">
        <div class="p-4 bg-base-100">
            <span class="text-3xl">{{ sysStat.system.name }} {{ sysStat.system.version }}</span>
            <span v-if="sysStat.system.kernel">
                <br >
                {{ sysStat.system.kernel }}
            </span>
        </div>
        <div class="p-4 bg-base-100 flex items-center">
            <span v-if="sysStat.system.ffp_version">
                <strong>ffplayout:</strong>
                v{{ sysStat.system.ffp_version }}
            </span>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.cpu') }}</div>
            <div class="grid grid-cols-2">
                <div><strong>{{ $t('system.cores') }}:</strong> {{ sysStat.cpu.cores }}</div>
                <div><strong>{{ $t('system.usage') }}:</strong> {{ sysStat.cpu.usage.toFixed(2) }}%</div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.load') }}</div>
            <div class="grid grid-cols-3">
                <div>{{ sysStat.load.one }}</div>
                <div>{{ sysStat.load.five }}</div>
                <div>{{ sysStat.load.fifteen }}</div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.memory') }}</div>
            <div class="grid grid-cols-2">
                <div><strong>{{ $t('system.total') }}:</strong> {{ fileSize(sysStat.memory.total) }}</div>
                <div><strong>{{ $t('system.usage') }}:</strong> {{ fileSize(sysStat.memory.used) }}</div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.swap') }}</div>
            <div class="grid grid-cols-2">
                <div><strong>{{ $t('system.total') }}:</strong> {{ fileSize(sysStat.swap.total) }}</div>
                <div><strong>{{ $t('system.usage') }}:</strong> {{ fileSize(sysStat.swap.used) }}</div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">
                {{ $t('system.network') }} <span v-if="sysStat.network" class="fs-6">{{ sysStat.network?.name }}</span>
            </div>
            <div class="grid grid-cols-2">
                <div><strong>{{ $t('system.in') }}:</strong> {{ fileSize(sysStat.network?.current_in) }}</div>
                <div><strong>{{ $t('system.out') }}:</strong> {{ fileSize(sysStat.network?.current_out) }}</div>
                <div>{{ fileSize(sysStat.network?.total_in) }}</div>
                <div>{{ fileSize(sysStat.network?.total_out) }}</div>
            </div>
        </div>
        <div v-if="sysStat.storage?.path" class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.storage') }}</div>
            <div v-if="sysStat.storage"><strong>{{ $t('system.device') }}:</strong> {{ sysStat.storage?.path }}</div>

            <div v-if="sysStat.storage" class="grid grid-cols-2">
                <div><strong>{{ $t('system.size') }}:</strong> {{ fileSize(sysStat.storage?.total) }}</div>
                <div><strong>{{ $t('system.used') }}:</strong> {{ fileSize(sysStat.storage?.used) }}</div>
            </div>
        </div>
        <div v-else class="col-6 bg-primary p-2 border" />
    </div>
</template>
<script setup lang="ts">
const { fileSize } = stringFormatter()

const authStore = useAuth()
const configStore = useConfig()
const timer = ref()
const sysStat = ref({
    cpu: { cores: 0.0, usage: 0.0 },
    load: { one: 0.0, five: 0.0, fifteen: 0.0 },
    memory: { total: 0.0, used: 0.0, free: 0.0 },
    network: { name: '', current_in: 0.0, current_out: 0.0, total_in: 0.0, total_out: 0.0 },
    storage: { path: '', total: 0.0, used: 0.0 },
    swap: { total: 0.0, used: 0.0, free: 0.0 },
    system: { name: '', kernel: '', version: '', ffp_version: '' },
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

    if (!document?.hidden) {
        await $fetch<SystemStatistics>(`/api/system/${channel}`, {
            method: 'GET',
            headers: { ...configStore.contentType, ...authStore.authHeader },
        }).then((stat) => {
            sysStat.value = stat
        })
    }
}
</script>
