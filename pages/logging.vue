<template>
    <div>
        <Menu />
        <div class="date-container">
            <div class="date-div">
                <input type="date" class="form-control" v-model="listDate" />
            </div>
        </div>
        <div class="log-container mt-2">
            <div class="log-content" v-html="formatLog(currentLog)" />
        </div>
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

useHead({
    title: 'Logging | ffplayout'
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
.date-container {
    width: 100%;
    height: 37px;
}
.log-container {
    background: $bg-secondary;
    height: calc(100% - 120px);
    margin: 1em;
    padding: .5em;
    overflow: hidden;
}

.log-time {
    color: $log-time;
}

.log-number {
    color: $log-number;
}

.log-addr {
    color: $log-addr ;
    font-weight: 500;
}

.log-cmd {
    color: $log-cmd;
}

.log-content {
    color: $log-content;
    width: 100%;
    height: 100%;
    font-family: monospace;
    font-size: 13px;
    white-space: pre;
    overflow: scroll;
    scrollbar-width: medium;
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
