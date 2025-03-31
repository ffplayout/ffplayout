<template>
    <div id="timeField" class="input input-sm flex px-3 py-0">
        <div class="grow">
            <input
                ref="timeInput"
                :value="secToTime(props.modelValue)"
                type="text"
                pattern="([01]?[0-9]|2[0-3]):[0-5][0-9]:[0-5][0-9](\.[0-9]{1,3})?"
                class="w-full px-1 py-0"
                @click="setCursorPos"
                @change="$emit('update:modelValue', timeToSec($event))"
            />
        </div>

        <div class="w-auto">
            <div class="flex flex-col text-xs py-[2px]">
                <button
                    class="bg-base-300 hover:bg-base-300/50 px-1 text-[9px] h-[13px] rounded-t"
                    tabindex="0"
                    @click="countUp"
                >
                    <i class="bi-chevron-up" />
                </button>
                <button class="bg-base-300 hover:bg-base-300/50 px-1 text-[9px] h-[13px] rounded-b" @click="countDown">
                    <i class="bi-chevron-down" />
                </button>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
import { ref, nextTick } from "vue"
const emit = defineEmits(['update:modelValue'])

const props = defineProps({
    modelValue: {
        type: Number,
        required: true,
    },
})

const timeInput = ref()
const cursorPos = ref(8)

function secToTime(sec: number) {
    const hours = Math.floor(sec / 3600)
    sec %= 3600
    const minutes = Math.floor(sec / 60)
    const seconds = Math.floor(sec % 60)
    const ms = Math.round((sec - Math.floor(sec)) * 1000) / 1000
    const secFmt = (seconds + ms).toFixed(3)

    const m = String(minutes).padStart(2, '0')
    const h = String(hours).padStart(2, '0')
    const s = secFmt.padStart(6, '0')

    return `${h}:${m}:${s}`
}

function timeToSec(event: any) {
    const time = event.target?.value ?? 0

    const [h, m, s] = time.split(':').map((val: string) => Number(val) || 0)

    return h * 3600 + m * 60 + s
}

function setCursorPos() {
    cursorPos.value = timeInput.value?.selectionStart
}

function countUp() {
    let count = 0

    if (cursorPos.value && cursorPos.value >= 6) {
        count = 1
    } else if (cursorPos.value && cursorPos.value >= 3) {
        count = 60
    } else {
        count = 3600
    }

    emit('update:modelValue', props.modelValue + count)

    nextTick(() => {
        timeInput.value?.focus()
        timeInput.value?.setSelectionRange(cursorPos.value, cursorPos.value)
    })
}

function countDown() {
    let sec = props.modelValue
    let count = 0

    if (cursorPos.value && cursorPos.value >= 6) {
        count = 1
    } else if (cursorPos.value && cursorPos.value >= 3) {
        count = 60
    } else {
        count = 3600
    }

    sec -= count

    if (sec < 0) {
        emit('update:modelValue', 0)
    } else {
        emit('update:modelValue', sec)
    }

    nextTick(() => {
        timeInput.value?.focus()
        timeInput.value?.setSelectionRange(cursorPos.value, cursorPos.value)
    })
}
</script>
<style scoped>
#timeField:has(> div > input:invalid) {
    border: red solid 1px;
}
</style>
