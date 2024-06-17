<template>
    <div class="flex w-full h-full ps-1">
        <div class="flex-none w-[70px] join join-vertical me-3 pt-7">
            <button
                class="join-item w-full btn btn-sm btn-primary duration-500"
                :class="activeConf === 1 && 'btn-secondary'"
                @click="activeConf = 1"
            >
                {{ $t('config.channel') }}
            </button>
            <button
                class="join-item w-full btn btn-sm btn-primary duration-500"
                :class="activeConf === 2 && 'btn-secondary'"
                @click="activeConf = 2"
            >
                Advanced
            </button>
            <button
                class="join-item w-full btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 3 && 'btn-secondary'"
                @click="activeConf = 3"
            >
                Playout
            </button>
            <button
                class="join-item w-full btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 4 && 'btn-secondary'"
                @click="activeConf = 4"
            >
                {{ $t('config.user') }}
            </button>
        </div>
        <div class="w-[calc(100%-70px)] mt-10 px-6">
            <div v-if="activeConf === 1" class="w-full flex justify-center">
                <ConfigChannel />
            </div>

            <div v-if="activeConf === 2" class="w-full flex justify-center">
                <ConfigAdvanced />
            </div>

            <div v-else-if="activeConf === 3" class="w-full flex justify-center">
                <ConfigPlayout />
            </div>

            <div v-else-if="activeConf === 4" class="w-full flex justify-center">
                <ConfigUser />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
const { t } = useI18n()

useHead({
    title: `${t('button.configure')} | ffplayout`,
})

const activeConf = ref(1)
</script>
