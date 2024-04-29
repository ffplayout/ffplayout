<template>
    <div class="w-full flex flex-col">
        <div class="flex justify-end p-3 h-14">
            <div class="join">
                <VueDatePicker
                    v-model="listDate"
                    :clearable="false"
                    :hide-navigation="['time']"
                    :action-row="{ showCancel: false, showSelect: false, showPreview: false }"
                    :format="calendarFormat"
                    model-type="yyyy-MM-dd"
                    auto-apply
                    :locale="locale"
                    :dark="colorMode.value === 'dark'"
                    input-class-name="join-item input input-sm !input-bordered !w-[300px] text-right !pe-3"
                    required
                />
                <button
                    class="btn btn-sm btn-primary join-item"
                    :title="$t('log.download')"
                    @click="downloadLog"
                >
                    <i class="bi-download" />
                </button>
            </div>
        </div>
        <div class="px-3 inline-block h-[calc(100vh-140px)] text-[13px]">
            <div class="bg-base-300 whitespace-pre h-full font-mono overflow-auto p-3" v-html="formatLog(currentLog)" />
        </div>
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const colorMode = useColorMode()
const { locale, t } = useI18n()

useHead({
    title: `${t('button.logging')} | ffplayout`,
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
    return $dayjs(date).locale(locale.value).format('dddd - LL')
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

function downloadLog() {
    const file = new File(
        [formatLog(currentLog.value).replace(/<\/?[^>]+(>|$)/g, '')],
        `playout_${listDate.value}.log`,
        {
            type: 'text/plain',
        }
    )

    const link = document.createElement('a')
    const url = URL.createObjectURL(file)

    link.href = url
    link.download = file.name
    document.body.appendChild(link)
    link.click()

    document.body.removeChild(link)
    window.URL.revokeObjectURL(url)
}
</script>

<style>
.log-time {
    color: #666864;
}

.log-number {
    color: var(--my-yellow);
}

.log-addr {
    color: var(--my-purple);
    font-weight: 500;
}

.log-cmd {
    color: var(--my-blue);
}

.log-info {
    color: var(--my-green);
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
