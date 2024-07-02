<template>
    <div class="w-full max-w-[800px]">
        <h2 class="pt-3 text-3xl">{{ $t('config.channelConf') }}</h2>
        <div class="w-full flex justify-end my-4">
            <button v-if="authStore.role === 'GlobalAdmin'" class="btn btn-sm btn-primary" @click="addChannel()">
                {{ $t('config.addChannel') }}
            </button>
        </div>
        <form
            v-if="configStore.configChannel && configStore.configChannel[configStore.configID]"
            class="w-full"
            @submit.prevent="onSubmitGui"
        >
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">{{ $t('config.name') }}</span>
                </div>
                <input
                    v-model="configStore.configChannel[configStore.configID].name"
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ $t('config.previewUrl') }}</span>
                </div>
                <input
                    v-model="configStore.configChannel[configStore.configID].preview_url"
                    type="text"
                    class="input input-bordered w-full"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ $t('config.extensions') }}</span>
                </div>
                <input
                    v-model="configStore.configChannel[configStore.configID].extra_extensions"
                    type="text"
                    class="input input-bordered w-full"
                />
            </label>

            <div class="join my-4">
                <button class="join-item btn btn-primary" type="submit">{{ $t('config.save') }}</button>
                <button
                    v-if="
                        authStore.role === 'GlobalAdmin' &&
                        configStore.configChannel.length > 1 &&
                        configStore.configChannel[configStore.configID].id > 1
                    "
                    class="join-item btn btn-primary"
                    @click="deleteChannel()"
                >
                    {{ $t('config.delete') }}
                </button>
            </div>
        </form>
    </div>
</template>

<script setup lang="ts">
const { $_ } = useNuxtApp()
const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

async function addChannel() {
    const channels = $_.cloneDeep(configStore.configChannel)
    const newChannel = $_.cloneDeep(configStore.configChannel[configStore.configChannel.length - 1])

    newChannel.id = channels.length + 1
    newChannel.name = `Channel ${Math.random().toString(36).substring(7)}`
    newChannel.preview_url = `${window.location.protocol}//${window.location.host}/live/${newChannel.id}/stream.m3u8`

    channels.push(newChannel)
    configStore.configChannel = channels
    configStore.configID = configStore.configChannel.length - 1
}

async function onSubmitGui() {
    /*
        Save GUI settings.
    */
    const update = await configStore.setChannelConfig(configStore.configChannel[configStore.configID])

    if (update.status) {
        indexStore.msgAlert('success', t('config.updateChannelSuccess'), 2)
    } else {
        indexStore.msgAlert('error', t('config.updateChannelFailed'), 2)
    }
}

async function deleteChannel() {
    const config = $_.cloneDeep(configStore.configChannel)
    const id = config[configStore.configID].id

    if (id === 1) {
        indexStore.msgAlert('warning', t('config.errorChannelDelete'), 2)
        return
    }

    const response = await fetch(`/api/channel/${id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })

    config.splice(configStore.configID, 1)
    configStore.configChannel = config
    configStore.configID = configStore.configChannel.length - 1
    await configStore.getPlayoutConfig()

    if (response.status === 200) {
        indexStore.msgAlert('success', t('config.errorChannelDelete'), 2)
    } else {
        indexStore.msgAlert('error', t('config.deleteChannelFailed'), 2)
    }
}
</script>
