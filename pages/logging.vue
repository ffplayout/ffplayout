<template>
    <div class="flex justify-end my-3 pe-3">
        <div>
            <input type="date" class="input input-sm input-bordered w-full max-w-xs" v-model="listDate" />
        </div>
    </div>
    <div class="px-3 inline-block h-[calc(100vh-130px)] text-[13px]">
        <div class="bg-base-300 whitespace-pre h-full font-mono overflow-auto p-3" v-html="formatLog(currentLog)" />
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

useHead({
    title: 'Logging | ffplayout',
})

const { configID } = storeToRefs(useConfig())

const { $dayjs } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const currentLog = ref('')
const listDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const { formatLog } = stringFormatter()

onMounted(() => {
    getLog()
})

watch([listDate, configID], () => {
    getLog()
})

async function getLog() {
    let date = listDate.value

    if (date === $dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD')) {
        date = ''
    }

    await fetch(`/api/log/${configStore.configGui[configStore.configID].id}?date=${date}`, {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.text())
        .then((data) => {
            currentLog.value = data
        })
        .catch(() => {
            currentLog.value = ''
        })
}
</script>

<style lang="scss">
.log-time {
    color: $log-time;
}

.log-number {
    color: $log-number;
}

.log-addr {
    color: $log-addr;
    font-weight: 500;
}

.log-cmd {
    color: $log-cmd;
}

.log-info {
    color: $log-info;
}

.log-warning {
    color: $log-warning;
}

.log-error {
    color: $log-error;
}

.log-debug {
    color: $log-debug;
}

.log-decoder {
    color: $log-decoder;
}

.log-encoder {
    color: $log-encoder;
}

.log-server {
    color: $log-server;
}
</style>
