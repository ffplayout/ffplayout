<template>
    <div class="max-w-[1200px] pe-8">
        <h2 class="pt-3 text-3xl">{{ $t('config.playoutConf') }}</h2>
        <form
            v-if="configStore.playout"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitPlayout"
        >
            <template v-for="(item, key) in configStore.playout" :key="key">
                <div class="text-xl pt-3 text-right">{{ setTitle(key.toString()) }}:</div>
                <div class="md:pt-4">
                    <label
                        v-for="(prop, name) in (item as Record<string, any>)"
                        :key="name"
                        class="form-control w-full"
                        :class="[typeof prop === 'boolean' && 'flex-row', name.toString() !== 'help_text' && 'mt-2']"
                    >
                        <template v-if="name.toString() !== 'startInSec' && name.toString() !== 'lengthInSec'">
                            <div v-if="name.toString() !== 'help_text'" class="label">
                                <span class="label-text !text-md font-bold">{{ name }}</span>
                            </div>
                            <div v-if="name.toString() === 'help_text'" class="whitespace-pre-line">
                                {{ setHelp(key.toString(), prop) }}
                            </div>
                            <input
                                v-else-if="name.toString() === 'sender_pass'"
                                v-model="item[name]"
                                type="password"
                                :placeholder="$t('config.placeholderPass')"
                                class="input input-sm input-bordered w-full"
                            />
                            <textarea
                                v-else-if="name.toString() === 'output_param' || name.toString() === 'custom_filter'"
                                v-model="item[name]"
                                class="textarea textarea-bordered"
                                rows="3"
                            />
                            <input
                                v-else-if="typeof prop === 'number' && prop % 1 === 0"
                                v-model="item[name]"
                                type="number"
                                class="input input-sm input-bordered w-full"
                            />
                            <input
                                v-else-if="typeof prop === 'number'"
                                v-model="item[name]"
                                type="number"
                                class="input input-sm input-bordered w-full"
                                step="0.0001"
                                style="max-width: 250px"
                            />
                            <input
                                v-else-if="typeof prop === 'boolean'"
                                v-model="item[name]"
                                type="checkbox"
                                class="checkbox checkbox-sm ms-2 mt-2"
                            />
                            <input
                                v-else-if="name === 'ignore_lines'"
                                v-model="formatIgnoreLines"
                                type="text"
                                class="input input-sm input-bordered w-full"
                            />
                            <input
                                v-else
                                :id="name"
                                v-model="item[name]"
                                type="text"
                                class="input input-sm input-bordered w-full"
                            />
                        </template>
                    </label>
                </div>
            </template>
            <div class="mt-5 mb-10">
                <button class="btn btn-primary" type="submit">{{ $t('config.save') }}</button>
            </div>
        </form>
    </div>

    <GenericModal
        :title="$t('config.restartTile')"
        :text="$t('config.restartText')"
        :show="showModal"
        :modal-action="restart"
    />
</template>

<script setup lang="ts">
const { t } = useI18n()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const showModal = ref(false)

const formatIgnoreLines = computed({
    get() {
        return configStore.playout.logging.ignore_lines.join(';')
    },

    set(value) {
        configStore.playout.logging.ignore_lines = value.split(';')
    },
})

function setTitle(input: string): string {
    switch (input) {
        case 'general':
            return t('config.general')
        case 'rpc_server':
            return t('config.rpcServer')
        case 'mail':
            return t('config.mail')
        case 'logging':
            return t('config.logging')
        case 'processing':
            return t('config.processing')
        case 'ingest':
            return t('config.ingest')
        case 'playlist':
            return t('config.playlist')
        case 'storage':
            return t('config.storage')
        case 'text':
            return t('config.text')
        case 'task':
            return t('config.task')
        case 'out':
            return t('config.out')
        default:
            return input
    }
}

function setHelp(key: string, text: string): string {
    switch (key) {
        case 'general':
            return t('config.generalText')
        case 'rpc_server':
            return t('config.rpcText')
        case 'mail':
            return t('config.mailText')
        case 'logging':
            return t('config.logText')
        case 'processing':
            return t('config.processingText')
        case 'ingest':
            return t('config.ingestText')
        case 'playlist':
            return t('config.playlistText')
        case 'storage':
            return t('config.storageText')
        case 'text':
            return t('config.textText')
        case 'task':
            return t('config.taskText')
        case 'out':
            return t('config.outText')
        default:
            return text
    }
}

async function onSubmitPlayout() {
    const update = await configStore.setPlayoutConfig(configStore.playout)
    configStore.onetimeInfo = true

    if (update.status === 200) {
        indexStore.msgAlert('success', 'Update playout config success!', 2)

        const channel = configStore.configGui[configStore.configID].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'status' }),
        }).then((response: any) => {
            if (response === 'active') {
                showModal.value = true
            }
        })
    } else {
        indexStore.msgAlert('error', 'Update playout config failed!', 2)
    }
}

async function restart(res: boolean) {
    if (res) {
        const channel = configStore.configGui[configStore.configID].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'restart' }),
        })
    }

    showModal.value = false
}
</script>
