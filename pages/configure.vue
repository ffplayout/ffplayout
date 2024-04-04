<template>
    <div class="flex w-full h-full">
        <div class="flex-none w-[70px] me-3 px-1 pt-7">
            <button
                class="w-full btn btn-sm btn-primary duration-500"
                :class="activeConf === 1 && 'btn-secondary'"
                @click="activeConf = 1"
            >
                GUI
            </button>
            <button
                class="w-full btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 2 && 'btn-secondary'"
                @click="activeConf = 2"
            >
                Playout
            </button>
            <button
                class="w-full btn btn-sm btn-primary mt-1 duration-500"
                :class="activeConf === 3 && 'btn-secondary'"
                @click="activeConf = 3"
            >
                User
            </button>
        </div>
        <div class="w-[calc(100%-70px)] mt-10 px-6">
            <div v-if="activeConf === 1" class="w-full flex justify-center">
                <ConfigGui />
            </div>

            <div v-else-if="activeConf === 2" class="w-full flex justify-center">
                <ConfigPlayout />
            </div>

            <div v-else-if="activeConf === 3" class="w-full flex justify-center">
                <ConfigUser />
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
useHead({
    title: 'Configuration | ffplayout',
})

const activeConf = ref(1)
</script>

