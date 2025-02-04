<template>
    <div v-if="channel" class="w-full max-w-[800px]">
        <h2 class="pt-3 text-3xl">{{ t('config.channelConf') }} ({{ channel.id }})</h2>
        <div class="w-full flex justify-end my-4">
            <button v-if="authStore.role === 'global_admin'" class="btn btn-sm btn-primary" @click="newChannel()">
                {{ t('config.addChannel') }}
            </button>
        </div>
        <div class="w-full">
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">{{ t('config.name') }}</span>
                </div>
                <input
                    v-model="channel.name"
                    type="text"
                    name="name"
                    placeholder="Type here"
                    class="input input-bordered w-full !bg-base-100"
                    @keyup="isChanged"
                    :disabled="authStore.role === 'user'"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.previewUrl') }}</span>
                </div>
                <input
                    v-model="channel.preview_url"
                    type="text"
                    name="preview_url"
                    class="input input-bordered w-full !bg-base-100"
                    @keyup="isChanged"
                    :disabled="authStore.role === 'user'"
                />
            </label>

            <label class="form-control w-full mt-5">
                <div class="label">
                    <span class="label-text">{{ t('config.extensions') }}</span>
                </div>
                <input
                    v-model="channel.extra_extensions"
                    type="text"
                    name="extra_extensions"
                    class="input input-bordered w-full !bg-base-100"
                    @keyup="isChanged"
                    :disabled="authStore.role === 'user'"
                />
            </label>

            <template v-if="authStore.role === 'global_admin'">
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
                        v-model="channel.public"
                        type="text"
                        name="public"
                        class="input input-bordered w-full"
                        @keyup="isChanged"
                    />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.playlistPath') }}</span>
                    </div>
                    <input
                        v-model="channel.playlists"
                        type="text"
                        name="playlists"
                        class="input input-bordered w-full"
                        @keyup="isChanged"
                    />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.storagePath') }}</span>
                    </div>
                    <input
                        v-model="channel.storage"
                        type="text"
                        name="storage"
                        class="input input-bordered w-full"
                        @keyup="isChanged"
                    />
                </label>

                <label class="form-control w-full mt-5">
                    <div class="label">
                        <span class="label-text">{{ t('config.timezone') }}</span>
                    </div>
                    <select
                        v-model="channel.timezone"
                        class="select select-md select-bordered w-full max-w-xs"
                        @change="isChanged"
                    >
                        <option v-for="zone in Intl.supportedValuesOf('timeZone')" :key="zone" :value="zone">
                            {{ zone }}
                        </option>
                    </select>
                </label>
            </template>

            <div v-if="authStore.role !== 'user'" class="my-5 flex gap-1">
                <button class="btn" :class="saved ? 'btn-primary' : 'btn-error'" @click="addUpdateChannel()">
                    {{ t('config.save') }}
                </button>
                <button
                    v-if="
                        authStore.role === 'global_admin' && configStore.channels.length > 1 && channel.id > 1 && saved
                    "
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
        <GenericModal
            :title="t('config.restartTile')"
            :text="t('config.restartText')"
            :show="configStore.showRestartModal"
            :modal-action="configStore.restart"
        />
    </div>
</template>

<script setup lang="ts">
import dayjs from 'dayjs'
import { cloneDeep, isEqual } from 'lodash-es'

const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const mediaStore = useMedia()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())

const saved = ref(true)
const channel = ref({} as Channel)
const channelOrig = ref({} as Channel)

onMounted(() => {
    channel.value = cloneDeep(configStore.channels[i.value])
    channelOrig.value = cloneDeep(configStore.channels[i.value])
})

watch([i], () => {
    if (configStore.channels[i.value]) {
        channel.value = cloneDeep(configStore.channels[i.value])
    }
})

function isChanged() {
    if (isEqual(channel.value, channelOrig.value)) {
        saved.value = true
    } else {
        saved.value = false
    }
}

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
    channel.value.timezone = dayjs.tz.guess()

    saved.value = false
}

async function addNewChannel() {
    await $fetch('/api/channel/', {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(channel.value),
    })
        .then((chl) => {
            i.value = channel.value.id - 1
            configStore.channels.push(cloneDeep(chl as Channel))
            configStore.channelsRaw.push(chl as Channel)
            configStore.configCount = configStore.channels.length
            configStore.timezone = channel.value.timezone || 'UTC'

            indexStore.msgAlert('success', t('config.updateChannelSuccess'), 2)
        })
        .catch(() => {
            indexStore.msgAlert('error', t('config.updateChannelFailed'), 3)
        })
}

async function updateChannel() {
    await fetch(`/api/channel/${channel.value.id}`, {
        method: 'PATCH',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(channel.value),
    })
        .then(() => {
            const oldTimezone = configStore.timezone
            const currentTimezone = channel.value.timezone

            for (let i = 0; i < configStore.channels.length; i++) {
                if (configStore.channels[i].id === channel.value.id) {
                    configStore.channels[i] = cloneDeep(channel.value)
                    configStore.timezone = channel.value.timezone || 'UTC'
                    break
                }
            }

            for (let i = 0; i < configStore.channelsRaw.length; i++) {
                if (configStore.channelsRaw[i].id === channel.value.id) {
                    configStore.channelsRaw[i] = cloneDeep(channel.value)
                    break
                }
            }

            channel.value = cloneDeep(configStore.channels[i.value])
            channelOrig.value = cloneDeep(configStore.channels[i.value])

            if (oldTimezone !== currentTimezone) {
                configStore.showRestartModal = true
            }

            indexStore.msgAlert('success', t('config.updateChannelSuccess'), 2)
        })
        .catch(() => {
            indexStore.msgAlert('error', t('config.updateChannelFailed'), 3)
        })
}

async function addUpdateChannel() {
    /*
        Save or update channel settings.
    */
    if (!saved.value) {
        saved.value = true

        if (configStore.channels[i.value].id !== channel.value.id) {
            await addNewChannel()
        } else {
            await updateChannel()
        }

        if (authStore.role === 'global_admin') {
            await configStore.getAdvancedConfig()
        }

        await configStore.getPlayoutConfig()
        await configStore.getUserConfig()
        await mediaStore.getTree('')
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

    if (authStore.role === 'global_admin') {
        await configStore.getAdvancedConfig()
    }

    await configStore.getChannelConfig()
    await configStore.getPlayoutConfig()
    await configStore.getUserConfig()
    await mediaStore.getTree('')

    if (response.status === 200) {
        indexStore.msgAlert('success', t('config.errorChannelDelete'), 2)
    } else {
        indexStore.msgAlert('error', t('config.deleteChannelFailed'), 2)
    }
}
</script>
