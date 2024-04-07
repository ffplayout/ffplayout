<template>
    <div v-if="show" class="z-50 fixed top-0 bottom-0 left-0 right-0 flex justify-center items-center bg-black/30">
        <div class="flex flex-col bg-base-100 min-w-[400px] max-w-[90%] h-auto rounded-md p-5 shadow-xl">
            <div class="font-bold text-lg">{{ title }}</div>

            <div class="grow mt-3">
                <slot>
                    <div v-html="text" />
                </slot>
            </div>

            <div class="flex justify-end mt-3">
                <div class="join">
                    <button class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item" @click="modalAction(false)">
                        Cancel
                    </button>
                    <button class="btn btn-sm bg-base-300 hover:bg-base-300/50 join-item" @click="modalAction(true)">
                        Ok
                    </button>
                </div>
            </div>
        </div>
    </div>
</template>
<script setup lang="ts">
defineProps({
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
})
</script>
