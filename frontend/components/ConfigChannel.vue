<template>
    <div v-if="configStore.channels && configStore.channels[configStore.id]" class="w-full max-w-[800px]">
        <h2 class="pt-3 text-3xl">{{ t('config.channelConf') }} ({{ configStore.channels[configStore.id].id }})</h2>
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
                <input
                    v-model="configStore.channels[configStore.id].name"
                    type="text"
                    placeholder="Type here"
                    class="input input-bordered w-full"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.previewUrl') }}</span>
                </div>
                <input
                    v-model="configStore.channels[configStore.id].preview_url"
                    type="text"
                    class="input input-bordered w-full"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.extensions') }}</span>
                </div>
                <input
                    v-model="configStore.channels[configStore.id].extra_extensions"
                    type="text"
                    class="input input-bordered w-full"
                />
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
                    <input
                        v-model="configStore.channels[configStore.id].public"
                        type="text"
                        class="input input-bordered w-full"
                    />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.playlistPath') }}</span>
                    </div>
                    <input
                        v-model="configStore.channels[configStore.id].playlists"
                        type="text"
                        class="input input-bordered w-full"
                    />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.storagePath') }}</span>
                    </div>
                    <input
                        v-model="configStore.channels[configStore.id].storage"
                        type="text"
                        class="input input-bordered w-full"
                    />
                </label>
            </template>

            <div class="join my-4">
                <button class="join-item btn btn-primary" :class="saved ? 'btn-primary' : 'btn-error'" @click="addUpdateChannel()">
                    {{ t('config.save') }}
                </button>
                <button
                    v-if="
                        authStore.role === 'GlobalAdmin' &&
                        configStore.channels.length > 1 &&
                        configStore.channels[configStore.id].id > 1
                    "
                    class="join-item btn btn-primary"
                    @click="deleteChannel()"
                >
                    {{ t('config.delete') }}
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

const saved = ref(true)

function rmId(path: string) {
    return path.replace(/\/\d+$/, '')
}

function newChannel() {
    const channels = cloneDeep(configStore.channels)
    const newChannel = cloneDeep(configStore.channels[configStore.channels.length - 1])

    newChannel.id = channels.length + 1
    newChannel.name = `Channel ${newChannel.id}`
    newChannel.preview_url = `${window.location.protocol}//${window.location.host}/${newChannel.id}/live/stream.m3u8`
    newChannel.public = `${rmId(newChannel.public)}/${newChannel.id}`
    newChannel.playlists = `${rmId(newChannel.playlists)}/${newChannel.id}`
    newChannel.storage = `${rmId(newChannel.storage)}/${newChannel.id}`

    channels.push(newChannel)
    configStore.channels = channels
    configStore.id = configStore.channels.length - 1

    saved.value = false
}

async function addUpdateChannel() {
    /*
        Save channel settings.
    */
    const update = await configStore.setChannelConfig(configStore.channels[configStore.id])

    if (update.status && update.status < 400) {
        indexStore.msgAlert('success', t('config.updateChannelSuccess'), 2)
        saved.value = true
    } else {
        indexStore.msgAlert('error', t('config.updateChannelFailed'), 2)
    }
}

async function deleteChannel() {
    const config = cloneDeep(configStore.channels)
    const id = config[configStore.id].id

    if (id === 1) {
        indexStore.msgAlert('warning', t('config.errorChannelDelete'), 2)
        return
    }

    const response = await fetch(`/api/channel/${id}`, {
        method: 'DELETE',
        headers: authStore.authHeader,
    })

    config.splice(configStore.id, 1)
    configStore.channelsRaw.splice(configStore.id, 1)
    configStore.channels = config
    configStore.id = configStore.channels.length - 1

    await configStore.getPlayoutConfig()

    if (response.status === 200) {
        indexStore.msgAlert('success', t('config.errorChannelDelete'), 2)
    } else {
        indexStore.msgAlert('error', t('config.deleteChannelFailed'), 2)
    }
}
</script>
