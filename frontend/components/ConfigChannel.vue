<template>
    <div v-if="channel" class="w-full max-w-[800px]">
        <h2 class="pt-3 text-3xl">{{ t('config.channelConf') }} ({{ channel.id }})</h2>
        <div class="w-full flex justify-end my-4">
            <button v-if="authStore.role === 'GlobalAdmin'" class="btn btn-sm btn-primary" @click="newChannel()">
                {{ t('config.addChannel') }}
            </button>
        </div>
        <div class="w-full">
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">{{ t('config.name') }}</span>
                </div>
                <input v-model="channel.name" type="text" placeholder="Type here" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.previewUrl') }}</span>
                </div>
                <input v-model="channel.preview_url" type="text" class="input input-bordered w-full" />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.extensions') }}</span>
                </div>
                <input v-model="channel.extra_extensions" type="text" class="input input-bordered w-full" />
            </label>

            <template v-if="authStore.role === 'GlobalAdmin'">
                <div class="mt-7 font-bold h-3">
                    <p v-if="configStore.playout.storage.shared_storage">
                        <SvgIcon name="warning" classes="inline mr-2" />
                        <span>{{ t('config.sharedStorage') }}</span>
                    </p>
                </div>
                <label class="form-control w-full mt-3">
                    <div class="label">
                        <span class="label-text">{{ t('config.publicPath') }}</span>
                    </div>
                    <input v-model="channel.public" type="text" class="input input-bordered w-full" />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.playlistPath') }}</span>
                    </div>
                    <input v-model="channel.playlists" type="text" class="input input-bordered w-full" />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.storagePath') }}</span>
                    </div>
                    <input v-model="channel.storage" type="text" class="input input-bordered w-full" />
                </label>
            </template>

            <div class="my-4 flex gap-1">
                <button class="btn" :class="saved ? 'btn-primary' : 'btn-error'" @click="addUpdateChannel()">
                    {{ t('config.save') }}
                </button>
                <button
                    v-if="authStore.role === 'GlobalAdmin' && configStore.channels.length > 1 && channel.id > 1 && saved"
                    class="btn btn-primary"
                    @click="deleteChannel()"
                >
                    {{ t('config.delete') }}
                </button>
                <button v-if="!saved" class="btn btn-primary text-xl" @click="resetChannel()">
                    <i class="bi-arrow-repeat" />
                </button>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
import { cloneDeep } from 'lodash-es'

const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())

const saved = ref(true)
const channel = ref({} as Channel)

onMounted(() => {
    channel.value = cloneDeep(configStore.channels[i.value])
})

watch([i], () => {
    if (configStore.channels[i.value]) {
        channel.value = cloneDeep(configStore.channels[i.value])
    }
})

function rmId(path: string) {
    return path.replace(/\/\d+$/, '')
}

function newChannel() {
    channel.value.id = configStore.channels.length + 1
    channel.value.name = `Channel ${channel.value.id}`
    channel.value.preview_url = `${window.location.protocol}//${window.location.host}/${channel.value.id}/live/stream.m3u8`
    channel.value.public = `${rmId(channel.value.public)}/${channel.value.id}`
    channel.value.playlists = `${rmId(channel.value.playlists)}/${channel.value.id}`
    channel.value.storage = `${rmId(channel.value.storage)}/${channel.value.id}`

    saved.value = false
}

async function addUpdateChannel() {
    /*
        Save channel settings.
    */
    saved.value = true
    i.value = channel.value.id - 1
    configStore.channels.push(cloneDeep(channel.value))
    const update = await configStore.setChannelConfig(channel.value)

    if (update.status && update.status < 400) {
        indexStore.msgAlert('success', t('config.updateChannelSuccess'), 2)

        await configStore.getPlayoutConfig()
        await configStore.getUserConfig()

    } else {
        indexStore.msgAlert('error', t('config.updateChannelFailed'), 2)
    }
}

function resetChannel() {
    channel.value = cloneDeep(configStore.channels[i.value])
    saved.value = true
}

async function deleteChannel() {
    if (channel.value.id === 1) {
        indexStore.msgAlert('warning', t('config.errorChannelDelete'), 2)
        return
    }

    const response = await fetch(`/api/channel/${channel.value.id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })

    i.value = configStore.i - 1
    await configStore.getChannelConfig()
    await configStore.getPlayoutConfig()
    await configStore.getUserConfig()

    if (response.status === 200) {
        indexStore.msgAlert('success', t('config.errorChannelDelete'), 2)
    } else {
        indexStore.msgAlert('error', t('config.deleteChannelFailed'), 2)
    }
}
</script>
