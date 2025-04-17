<template>
    <div v-if="show" class="z-50 fixed inset-0 flex justify-center bg-black/30 overflow-auto py-5">
        <div class="flex flex-col bg-base-100 min-w-[330px] w-auto max-w-[90%] rounded-md p-5 shadow-xl my-auto">
            <div class="inline-block">
                <div class="flex gap-2">
                    <div class="font-bold text-lg truncate flex-1 w-0">{{ title }}</div>
                    <button v-if="hideButtons" class="btn btn-sm w-8 h-8 rounded-full" @click="modalAction(false)">
                        <i class="bi bi-x-lg" />
                    </button>
                </div>

                <div class="grow mt-3">
                    <slot>
                        <div v-html="text" />
                    </slot>
                </div>
            </div>

            <div v-if="!hideButtons" class="flex justify-end mt-3">
                <div class="join">
                    <button class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item" @click="modalAction(false)">
                        {{ t('cancel') }}
                    </button>
                    <button class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item" @click="modalAction(true)">
                        {{ t('ok') }}
                    </button>
                </div>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
import { useI18n } from 'vue-i18n'
const { t } = useI18n()

const props = defineProps({
    title: {
        type: String,
        default: '',
    },
    text: {
        type: String,
        default: '',
    },
    modalAction: {
        type: Function,
        default() {
            return ''
        },
    },
    show: {
        type: Boolean,
        default: false,
    },
    hideButtons: {
        type: Boolean,
        default: false,
    },
})

document.body.style.overflow = props.show ? 'hidden' : ''
</script>
