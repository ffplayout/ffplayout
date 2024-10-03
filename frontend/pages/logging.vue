<template>
    <div class="w-full flex flex-col">
        <div class="flex justify-end p-3 h-14">
            <div class="join">
                <select v-model="errorLevel" class="join-item select select-sm select-bordered w-full max-w-xs">
                    <option
                        v-for="(index, value) in indexStore.severityLevels"
                        :key="index"
                        :value="value"
                        :selected="value === errorLevel"
                    >
                        {{ value }}
                    </option>
                </select>
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
                    :ui="{ input: 'join-item input !input-sm !input-bordered !w-[170px] text-right !pe-3' }"
                    required
                />
                <button class="btn btn-sm btn-primary join-item" :title="t('log.reload')" @click="getLog()">
                    <i class="bi-arrow-repeat" />
                </button>
                <button class="btn btn-sm btn-primary join-item" :title="t('log.download')" @click="downloadLog">
                    <i class="bi-download" />
                </button>
            </div>
        </div>
        <div class="px-3 inline-block h-[calc(100vh-140px)] text-[13px]">
            <div
                class="bg-base-300 whitespace-pre h-full font-mono overflow-auto p-3"
                v-html="filterLogsBySeverity(formatLog(currentLog), errorLevel)"
            />
        </div>
    </div>
</template>

<script setup lang="ts">
import { storeToRefs } from 'pinia'

const colorMode = useColorMode()
const { locale, t } = useI18n()

const indexStore = useIndex()

useHead({
    title: `${t('button.logging')} | ffplayout`,
})

const { i } = storeToRefs(useConfig())

const { $dayjs } = useNuxtApp()
const authStore = useAuth()
const configStore = useConfig()
const currentLog = ref('')
const listDate = ref($dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD'))
const { formatLog } = stringFormatter()

const levelCookie = useCookie('error_level', {
    path: '/',
    maxAge: 60 * 60 * 24 * 365,
    sameSite: 'lax',
})

const errorLevel = computed({
    get() {
        return levelCookie.value || 'INFO'
    },

    set(value) {
        levelCookie.value = value
    },
})

onMounted(async () => {
    await getLog()
})

watch([listDate, i], () => {
    getLog()
})

const calendarFormat = (date: Date) => {
   return $dayjs(date).locale(locale.value).format('ddd L')
}

function filterLogsBySeverity(logString: string, minSeverity: string): string {
    const minLevel = indexStore.severityLevels[minSeverity]
    const logLines = logString.trim().split(/\r?\n/)

    const filteredLogs = logLines.filter((log) => {
        const match = log.match(/\[ ?(DEBUG|INFO|WARN|ERROR)\]/)

        if (match) {
            const logLevel = match[1]
            return indexStore.severityLevels[logLevel] >= minLevel
        }
        return false
    })
    return filteredLogs.join('\n')
}

async function getLog() {
    let date = listDate.value

    if (date === $dayjs().utcOffset(configStore.utcOffset).format('YYYY-MM-DD')) {
        date = ''
    }

    await fetch(`/api/log/${configStore.channels[configStore.i].id}?date=${date}`, {
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
