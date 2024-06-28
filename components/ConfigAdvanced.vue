<template>
    <div class="min-w-[200px] pe-8 w-[768px]">
        <h2 class="pt-3 text-3xl">{{ $t('advanced.title') }}</h2>
        <p class="mt-5 font-bold text-orange-500">{{ $t('advanced.warning') }}</p>
        <form
            v-if="configStore.advanced"
            class="mt-10 grid md:grid-cols-[180px_auto] gap-5"
            @submit.prevent="onSubmitAdvanced"
        >
            <template v-for="(item, key) in configStore.advanced" :key="key">
                <div class="text-xl pt-3 text-right">{{ setTitle(key.toString()) }}:</div>
                <div class="md:pt-4">
                    <label
                        v-for="(_, name) in (item as Record<string, any>)"
                        :key="name"
                        class="form-control w-full"
                    >
                        <div class="label">
                            <span class="label-text !text-md font-bold">{{ name }}</span>
                        </div>
                        <input
                            :id="name"
                            v-model="item[name]"
                            type="text"
                            class="input input-sm input-bordered w-full"
                        />
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

function setTitle(input: string): string {
    switch (input) {
        case 'decoder':
            return t('advanced.decoder')
        case 'encoder':
            return t('advanced.encoder')
        case 'filter':
            return t('advanced.filter')
        case 'ingest':
            return t('advanced.ingest')
        default:
            return input
    }
}

async function onSubmitAdvanced() {
    const update = await configStore.setAdvancedConfig()
    configStore.onetimeInfo = true

    if (update.status === 200) {
        indexStore.msgAlert('success', t('advanced.updateSuccess'), 2)

        const channel = configStore.configChannel[configStore.configID].id

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
        indexStore.msgAlert('error', t('advanced.updateFailed'), 2)
    }
}

async function restart(res: boolean) {
    if (res) {
        const channel = configStore.configChannel[configStore.configID].id

        await $fetch(`/api/control/${channel}/process/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify({ command: 'restart' }),
        })
    }

    showModal.value = false
}
</script>
