<template>
    <!-- <div v-if="data">{{ systemStatus(data) }}</div> -->
    <div class="grid grid-cols-1 xs:grid-cols-2 border-4 rounded-md border-primary text-left shadow min-w-[728px] max-w-[960px]">
        <div class="p-4 bg-base-100">
            <span class="text-3xl">{{ sysStat.system.name }} {{ sysStat.system.version }}</span>
            <span v-if="sysStat.system.kernel">
                <br />
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
            <div class="grid grid-cols-2 gap-2">
                <div>
                    <strong>{{ $t('system.cores') }}:</strong> {{ sysStat.cpu.cores }}
                </div>
                <div>
                    <strong>{{ $t('system.usage') }}:</strong> {{ sysStat.cpu.usage.toFixed(2) }}%
                </div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.load') }}</div>
            <div class="grid grid-cols-3 gap-2">
                <div>{{ sysStat.load.one }}</div>
                <div>{{ sysStat.load.five }}</div>
                <div>{{ sysStat.load.fifteen }}</div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.memory') }}</div>
            <div class="grid grid-cols-2 gap-2">
                <div>
                    <strong>{{ $t('system.total') }}:</strong> {{ fileSize(sysStat.memory.total) }}
                </div>
                <div>
                    <strong>{{ $t('system.usage') }}:</strong> {{ fileSize(sysStat.memory.used) }}
                </div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.swap') }}</div>
            <div class="grid grid-cols-2 gap-2">
                <div>
                    <strong>{{ $t('system.total') }}:</strong> {{ fileSize(sysStat.swap.total) }}
                </div>
                <div>
                    <strong>{{ $t('system.usage') }}:</strong> {{ fileSize(sysStat.swap.used) }}
                </div>
            </div>
        </div>
        <div class="p-4 border border-primary">
            <div class="text-xl">
                {{ $t('system.network') }} <span v-if="sysStat.network" class="fs-6">{{ sysStat.network?.name }}</span>
            </div>
            <div class="grid grid-cols-2 gap-2">
                <div>
                    <strong>{{ $t('system.in') }}:</strong> {{ fileSize(sysStat.network?.current_in) }}
                </div>
                <div>
                    <strong>{{ $t('system.out') }}:</strong> {{ fileSize(sysStat.network?.current_out) }}
                </div>
                <div>{{ fileSize(sysStat.network?.total_in) }}</div>
                <div>{{ fileSize(sysStat.network?.total_out) }}</div>
            </div>
        </div>
        <div v-if="sysStat.storage?.path" class="p-4 border border-primary">
            <div class="text-xl">{{ $t('system.storage') }}</div>

            <div v-if="sysStat.storage" class="grid grid-cols-2 gap-2">
                <div v-if="sysStat.storage" class="col-span-2">
                    <strong>{{ $t('system.device') }}:</strong> {{ sysStat.storage?.path }}
                </div>
                <div>
                    <strong>{{ $t('system.size') }}:</strong> {{ fileSize(sysStat.storage?.total) }}
                </div>
                <div>
                    <strong>{{ $t('system.used') }}:</strong> {{ fileSize(sysStat.storage?.used) }}
                </div>
            </div>
        </div>
        <div v-else class="col-6 bg-primary p-2 border" />
    </div>
</template>
<script setup lang="ts">
const { fileSize } = stringFormatter()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const streamUrl = ref(
    `/data/event/${configStore.configGui[configStore.configID].id}?endpoint=system&uuid=${authStore.uuid}`
)

// 'http://127.0.0.1:8787/data/event/1?endpoint=system&uuid=f2f8c29b-712a-48c5-8919-b535d3a05a3a'
const { status, data, error, close } = useEventSource(streamUrl, [], {
    autoReconnect: {
        retries: -1,
        delay: 1000,
        onFailed() {
            indexStore.sseConnected = false
        },
    },
})

const errorCounter = ref(0)
const defaultStat = {
    cpu: { cores: 1, usage: 0.0 },
    load: { one: 0.0, five: 0.0, fifteen: 0.0 },
    memory: { total: 0.0, used: 0.0, free: 0.0 },
    network: { name: '...', current_in: 0.0, current_out: 0.0, total_in: 0.0, total_out: 0.0 },
    storage: { path: '', total: 0.0, used: 0.0 },
    swap: { total: 0.0, used: 0.0, free: 0.0 },
    system: { name: '...', kernel: '...', version: '...', ffp_version: '...' },
} as SystemStatistics

const sysStat = ref(defaultStat)

onBeforeUnmount(() => {
    close()
    indexStore.sseConnected = false
})

watch([status, error], async () => {
    if (status.value === 'OPEN') {
        indexStore.sseConnected = true
        errorCounter.value = 0
    } else {
        indexStore.sseConnected = false
        errorCounter.value += 1
        sysStat.value = defaultStat

        if (errorCounter.value > 15) {
            await authStore.obtainUuid()
            streamUrl.value = `/data/event/${configStore.configGui[configStore.configID].id}?endpoint=system&uuid=${
                authStore.uuid
            }`
            errorCounter.value = 0
        }
    }
})

watch([data], () => {
    if (data.value) {
        try {
            sysStat.value = JSON.parse(data.value)
        } catch (_) {
            indexStore.sseConnected = true
        }
    }
})
</script>
