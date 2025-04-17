<template>
    <div class="w-full flex flex-col">
        <div class="flex justify-end p-3 h-14">
            <div class="join">
                <select v-model="errorLevel" class="join-item select select-sm w-24">
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
                    class="max-w-[170px]"
                    :locale="locale"
                    :dark="indexStore.darkMode"
                    :ui="{ input: 'join-item input !input-sm !max-w-[170px] text-right !pe-3' }"
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
            <div id="log-container" class="bg-base-300 h-full font-mono overflow-auto p-3">
                <div
                    id="log-content"
                    class="whitespace-pre"
                    v-html="filterLogsBySeverity(currentLog, errorLevel)"
                />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat.js'
import LocalizedFormat from 'dayjs/plugin/localizedFormat.js'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import { computed, nextTick, ref, onMounted, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'
import VueDatePicker from '@vuepic/vue-datepicker'
import '@vuepic/vue-datepicker/dist/main.css'

import 'dayjs/locale/de'
import 'dayjs/locale/en'
import 'dayjs/locale/es'
import 'dayjs/locale/pt-br'
import 'dayjs/locale/ru'

dayjs.extend(customParseFormat)
dayjs.extend(LocalizedFormat)
dayjs.extend(timezone)
dayjs.extend(utc)

import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

const { locale, t } = useI18n()
const indexStore = useIndex()

useHead({
    title: computed(() => t('button.logging')),
})

const { i } = storeToRefs(useConfig())
const authStore = useAuth()
const configStore = useConfig()
const currentLog = ref('')
const listDate = ref(dayjs().tz(configStore.timezone).format('YYYY-MM-DD'))

const errorLevel = ref(localStorage.getItem('error_level') || 'INFO')

onMounted(async () => {
    await getLog()
})

watch([listDate, i], () => {
    getLog()
})

watch(errorLevel, (newValue) => {
    localStorage.setItem('error_level', newValue)
})

const calendarFormat = (date: Date) => {
    return dayjs(date).locale(locale.value).format('ddd L')
}

function scrollTo() {
    const parent = document.getElementById('log-container')
    const child = document.getElementById('log-content')

    if (child && parent) {
        parent.scrollTop = child.scrollHeight
    }
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
        return true
    })
    return filteredLogs.join('\n')
}

async function getLog() {
    let date = listDate.value

    if (date === dayjs().tz(configStore.timezone).format('YYYY-MM-DD')) {
        date = ''
    }

    await fetch(`/api/log/${configStore.channels[configStore.i].id}?date=${date}&timezone=${encodeURIComponent(configStore.timezone)}`, {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then(async (response) => {
            if (!response.ok) {
                throw new Error(await response.text())
            }
            return response.text()
        })
        .then((data) => {
            currentLog.value = data

            nextTick(() => {
                scrollTo()
            })
        })
        .catch(() => {
            currentLog.value = ''
        })
}

async function downloadLog() {
    let date = listDate.value
    let channel_id = configStore.channels[configStore.i].id

    if (date === dayjs().tz(configStore.timezone).format('YYYY-MM-DD')) {
        date = ''
    }

    const response = await fetch(`/api/log/${channel_id}?date=${date}&timezone=${encodeURIComponent(configStore.timezone)}&download=true`, {
        method: 'GET',
        headers: authStore.authHeader,
    })

    if (!response.ok) {
        indexStore.msgAlert('error', await response.text(), 7)
        return
    }

    if (date) {
        date = `_${date}`
    }

    const blob = await response.blob()
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')

    link.href = url
    link.download = `ffplayout_${channel_id}${date}.log`
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
    URL.revokeObjectURL(url)
}
</script>

<style>
.log-gray {
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

.level-info {
    color: var(--my-green);
}

.level-warning {
    color: var(--color-warning);
}

.level-error {
    color: var(--color-error);
}

.level-debug {
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

.log-validate {
    color: #23cbdd;
}
</style>
