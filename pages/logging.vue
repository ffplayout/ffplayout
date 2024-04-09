<template>
    <div class="flex justify-end p-3 h-14">
        <div>
            <VueDatePicker
                    v-model="listDate"
                    :clearable="false"
                    :hide-navigation="['time']"
                    :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                    :format="calendarFormat"
                    model-type="yyyy-MM-dd"
                    auto-apply
                    :dark="colorMode === 'dark'"
                    input-class-name="input input-sm !input-bordered !w-[230px] text-right !pe-3"
                    required
                />
        </div>
    </div>
    <div class="px-3 inline-block h-[calc(100vh-140px)] text-[13px]">
        <div class="bg-base-300 whitespace-pre h-full font-mono overflow-auto p-3" v-html="formatLog(currentLog)" />
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const colorMode = useColorMode()

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

const calendarFormat = (date: Date) => {
    return $dayjs(date).format('dddd DD. MMM YYYY')
}

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

<style>
.log-time {
    color: #666864;
}

.log-number {
    color: #e2c317;
}

.log-addr {
    color: #ad7fa8;
    font-weight: 500;
}

.log-cmd {
    color: #6c95c2;
}

.log-info {
    color: #8ae234;
}

.log-warning {
    color: #ff8700;
}

.log-error {
    color: #d32828;
}

.log-debug {
    color: #6e99c7;
}

.log-decoder {
    color: #56efff;
}

.log-encoder {
    color: #45ccee;
}

.log-server {
    color: #23cbdd;
}
</style>
