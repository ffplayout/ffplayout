<template>
    <div>
        <div class="flex flex-col items-center pt-10 px-8">
            <div class="mt-2 w-full max-w-4xl">
                <div class="flex flex-col xs:flex-row w-full join">
                    <div class="grow xs:max-w-72">
                        <select
                            v-model="selected"
                            class="select select-primary select-sm w-full"
                            @change="onChange($event)"
                        >
                            <option v-for="item in presets" :key="item.name">{{ item.name }}</option>
                        </select>
                    </div>
                    <button
                        class="btn btn-sm join-item btn-primary"
                        :title="t('message.savePreset')"
                        @click="savePreset()"
                    >
                        <i class="bi-cloud-upload" />
                    </button>
                    <button
                        class="btn btn-sm join-item btn-primary"
                        :title="t('message.newPreset')"
                        @click="showCreateModal = true"
                    >
                        <i class="bi-file-plus" />
                    </button>
                    <button
                        class="btn btn-sm join-item btn-primary"
                        :title="t('message.delPreset')"
                        @click="showDeleteModal = true"
                    >
                        <i class="bi-file-minus" />
                    </button>
                </div>

                <form class="my-6 w-full" @submit.prevent="submitMessage">
                    <textarea
                        v-model="form.text"
                        class="textarea w-full"
                        rows="6"
                        :placeholder="t('message.placeholder')"
                    />

                    <div class="mt-2 grid xs:grid-cols-[auto_150px_150px] gap-4">
                        <div class="grow">
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.xAxis') }}</legend>
                                <input v-model="form.x" type="text" name="x" class="input input-sm w-full" />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.yAxis') }}</legend>
                                <input v-model="form.y" type="text" name="y" class="input input-sm w-full" />
                            </fieldset>
                        </div>

                        <div class="xs:mt-8.5">
                            <fieldset class="fieldset rounded-box w-full">
                                <label class="fieldset-label text-base-content">
                                    <input v-model="form.showBox" type="checkbox" class="checkbox" />
                                    {{ t('message.showBox') }}
                                </label>
                            </fieldset>
                            <fieldset class="fieldset mt-1">
                                <legend class="fieldset-legend">{{ t('message.boxColor') }}</legend>
                                <input v-model="form.boxColor" type="color" class="input input-sm w-full cursor-pointer" />
                            </fieldset>
                        </div>
                        <fieldset class="fieldset mt-1 xs:mt-[70px]">
                            <legend class="fieldset-legend">{{ t('message.boxAlpha') }}</legend>
                            <input
                                v-model="form.boxAlpha"
                                type="number"
                                min="0"
                                max="1"
                                step="0.01"
                                class="input input-sm w-full"
                                required
                            />
                        </fieldset>
                    </div>
                    <div class="grid xs:grid-cols-[150px_150px_auto] gap-4 mt-2">
                        <div>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.size') }}</legend>
                                <input
                                    v-model="form.fontSize"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.fontColor') }}</legend>
                                <input v-model="form.fontColor" type="color" class="input input-sm w-full cursor-pointer" />
                            </fieldset>
                        </div>
                        <div>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.spacing') }}</legend>
                                <input
                                    v-model="form.fontSpacing"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.fontAlpha') }}</legend>
                                <input
                                    v-model="form.fontAlpha"
                                    type="number"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                        </div>

                        <div class="grow">
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.overallAlpha') }}</legend>
                                <input v-model="form.overallAlpha" type="text" name="overall_alpha" class="input input-sm w-full" required />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.borderWidth') }}</legend>
                                <input
                                    v-model="form.border"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                        </div>
                    </div>

                    <div class="mt-5">
                        <button class="btn btn-primary send-btn" type="submit">{{ t('message.send') }}</button>
                    </div>
                </form>
            </div>
        </div>

        <GenericModal :show="showCreateModal" :title="t('message.newPreset')" :modal-action="createNewPreset">
            <fieldset class="fieldset">
                <legend class="fieldset-legend">{{ t('message.name') }}</legend>
                <input v-model="newPresetName" type="text" name="overall_alpha" class="input input-sm w-full" required />
            </fieldset>
        </GenericModal>

        <GenericModal
            :show="showDeleteModal"
            :title="t('message.delPreset')"
            :text="`${t('message.delText')}: <strong> ${selected}</strong>?`"
            :modal-action="deletePreset"
        />
    </div>
</template>

<script setup lang="ts">
import { computed, ref, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { useHead } from '@unhead/vue'

import GenericModal from '@/components/GenericModal.vue'

import { stringFormatter } from '@/composables/helper'
import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())
const { numberToHex, hexToNumber } = stringFormatter()

useHead({
    title: computed(() => t('button.message')),
})

interface PresetName {
    name: string
    value: number
}

const form = ref({
    id: 0,
    name: '',
    text: '',
    x: '0',
    y: '0',
    fontSize: 24,
    fontSpacing: 4,
    fontColor: '#ffffff',
    fontAlpha: 1.0,
    showBox: true,
    boxColor: '#000000',
    boxAlpha: 0.8,
    border: 4,
    overallAlpha: '1',
})

const showCreateModal = ref(false)
const showDeleteModal = ref(false)
const selected = ref(null)
const newPresetName = ref('')
const presets = ref([] as PresetName[])

onMounted(() => {
    getPreset(-1)
})

watch([i], () => {
    nextTick(() => {
        getPreset(-1)
    })
})

async function getPreset(index: number) {
    fetch(`/api/presets/${configStore.channels[configStore.i]?.id}`, {
        method: 'GET',
        headers: authStore.authHeader,
    })
        .then((response) => response.json())
        .then((data) => {
            if (index === -1) {
                presets.value = [{ value: -1, name: '' }]

                for (let i = 0; i < data.length; i++) {
                    const elem = data[i]
                    presets.value.push({ value: i, name: elem.name })
                }

                form.value = {
                    id: 0,
                    name: '',
                    text: '',
                    x: '0',
                    y: '0',
                    fontSize: 24,
                    fontSpacing: 4,
                    fontColor: '#ffffff',
                    fontAlpha: 1.0,
                    showBox: true,
                    boxColor: '#000000',
                    boxAlpha: 0.8,
                    border: 4,
                    overallAlpha: '1',
                }
            } else {
                const fColor = data[index].fontcolor.split('@')
                const bColor = data[index].boxcolor.split('@')

                form.value = {
                    id: data[index].id,
                    name: data[index].name,
                    text: data[index].text,
                    x: data[index].x,
                    y: data[index].y,
                    fontSize: data[index].fontsize,
                    fontSpacing: data[index].line_spacing,
                    fontColor: fColor[0],
                    fontAlpha: fColor[1] ? hexToNumber(fColor[1]) : 1.0,
                    showBox: data[index].box === '1' ? true : false,
                    boxColor: bColor[0],
                    boxAlpha: bColor[1] ? hexToNumber(bColor[1]) : 1.0,
                    border: data[index].boxborderw,
                    overallAlpha: data[index].alpha,
                }
            }
        })
}

function onChange(event: any) {
    selected.value = event.target.value

    getPreset(event.target.selectedIndex - 1)
}

async function savePreset() {
    if (selected.value) {
        const preset = {
            id: form.value.id,
            name: form.value.name,
            text: form.value.text,
            x: form.value.x,
            y: form.value.y,
            fontsize: form.value.fontSize,
            line_spacing: form.value.fontSpacing,
            fontcolor:
                form.value.fontAlpha === 1
                    ? form.value.fontColor
                    : form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
            box: form.value.showBox ? '1' : '0',
            boxcolor:
                form.value.boxAlpha === 1
                    ? form.value.boxColor
                    : form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
            boxborderw: form.value.border,
            alpha: form.value.overallAlpha,
            channel_id: configStore.channels[configStore.i]?.id,
        }

        const response = await fetch(`/api/presets/${configStore.channels[configStore.i]?.id}/${form.value.id}`, {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify(preset),
        })

        if (response.status === 200) {
            indexStore.msgAlert('success', t('message.saveDone'), 2)
        } else {
            indexStore.msgAlert('error', t('message.saveFailed'), 2)
        }
    }
}

async function createNewPreset(create: boolean) {
    showCreateModal.value = false

    if (create) {
        const preset = {
            name: newPresetName.value,
            text: form.value.text,
            x: form.value.x.toString(),
            y: form.value.y.toString(),
            fontsize: form.value.fontSize.toString(),
            line_spacing: form.value.fontSpacing.toString(),
            fontcolor:
                form.value.fontAlpha === 1
                    ? form.value.fontColor
                    : form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
            box: form.value.showBox ? '1' : '0',
            boxcolor:
                form.value.boxAlpha === 1
                    ? form.value.boxColor
                    : form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
            boxborderw: form.value.border.toString(),
            alpha: form.value.overallAlpha.toString(),
            channel_id: configStore.channels[configStore.i]?.id,
        }

        const response = await fetch(`/api/presets/${configStore.channels[configStore.i]?.id}/`, {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify(preset),
        })

        if (response.status === 200) {
            indexStore.msgAlert('success', t('message.saveDone'), 2)
            getPreset(-1)
        } else {
            indexStore.msgAlert('error', t('message.saveFailed'), 2)
        }
    }

    newPresetName.value = ''
}

async function deletePreset(del: boolean) {
    showDeleteModal.value = false

    if (del && selected.value && selected.value !== '') {
        await fetch(`/api/presets/${configStore.channels[configStore.i]?.id}/${form.value.id}`, {
            method: 'DELETE',
            headers: authStore.authHeader,
        })

        getPreset(-1)
    }
}

async function submitMessage() {
    const obj = {
        text: form.value.text,
        x: form.value.x.toString(),
        y: form.value.y.toString(),
        fontsize: form.value.fontSize.toString(),
        line_spacing: form.value.fontSpacing.toString(),
        fontcolor: form.value.fontColor + '@' + numberToHex(form.value.fontAlpha),
        alpha: form.value.overallAlpha.toString(),
        box: form.value.showBox ? '1' : '0',
        boxcolor: form.value.boxColor + '@' + numberToHex(form.value.boxAlpha),
        boxborderw: form.value.border.toString(),
    }

    const response = await fetch(`/api/control/${configStore.channels[configStore.i]?.id}/text/`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(obj),
    })

    if (response.status === 200) {
        indexStore.msgAlert('success', t('message.sendDone'), 2)
    } else {
        indexStore.msgAlert('error', t('message.sendFailed'), 2)
    }
}
</script>
