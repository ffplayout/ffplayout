<script setup lang="ts">
import { computed, ref, nextTick, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import { useHead } from '@unhead/vue'

import GenericModal from '@/components/utils/GenericModal.vue'

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { i } = storeToRefs(useConfig())

useHead({
    title: computed(() => t('button.message')),
})

interface PresetName {
    name: string
    value: number
}

const defaultForm = (): TextPreset => ({
    id: 0,
    channel_id: configStore.channels[configStore.i]?.id ?? 1,
    name: '',
    text: '',
    use_filename: false,
    font_family: 'DejaVu Sans',
    font_weight: 'normal',
    filename_regex: '^.+[/\\\\](.*)(.mp4|.mkv|.webm)$',
    position_x: 'center',
    position_y: 'end:72',
    font_size: 24,
    line_spacing: 4,
    text_color: '#ffffff',
    text_opacity: 1,
    background_enabled: true,
    background_color: '#000000',
    background_opacity: 0.8,
    background_padding: 4,
    opacity: 1,
    scroll_direction: 'none',
    scroll_speed: 100,
    scroll_repeat: -1,
    fade_in_seconds: 0,
    fade_out_seconds: 0,
})
const form = ref<TextPreset>(defaultForm())

const showCreateModal = ref(false)
const showDeleteModal = ref(false)
const selected = ref(null)
const newPresetName = ref('')
const presets = ref([] as PresetName[])
const fontFamilies = ref<string[]>([])

onMounted(() => {
    getPreset(-1)
    getFontFamilies()
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

                form.value = defaultForm()
            } else {
                form.value = data[index]
            }
        })
}

async function getFontFamilies() {
    const response = await fetch('/api/text/fonts', {
        method: 'GET',
        headers: authStore.authHeader,
    })
    fontFamilies.value = response.ok ? await response.json() : []
}

function onChange(event: any) {
    selected.value = event.target.value

    getPreset(event.target.selectedIndex - 1)
}

async function savePreset() {
    if (selected.value) {
        const response = await fetch(`/api/presets/${configStore.channels[configStore.i]?.id}/${form.value.id}`, {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify(form.value),
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
            ...form.value,
            name: newPresetName.value,
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
    const response = await fetch(`/api/control/${configStore.channels[configStore.i]?.id}/text`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(form.value),
    })

    if (response.status === 200) {
        indexStore.msgAlert('success', t('message.sendDone'), 2)
    } else {
        indexStore.msgAlert('error', t('message.sendFailed'), 2)
    }
}
</script>
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

                    <div class="mt-2 grid md:grid-cols-[minmax(0,1fr)_160px_auto] gap-4">
                        <fieldset class="fieldset">
                            <legend class="fieldset-legend">Font</legend>
                            <select v-model="form.font_family" class="select select-sm w-full">
                                <option
                                    v-if="form.font_family && !fontFamilies.includes(form.font_family)"
                                    :value="form.font_family"
                                >
                                    {{ form.font_family }}
                                </option>
                                <option v-for="family in fontFamilies" :key="family" :value="family">
                                    {{ family }}
                                </option>
                            </select>
                        </fieldset>
                        <fieldset class="fieldset">
                            <legend class="fieldset-legend">Weight</legend>
                            <select v-model="form.font_weight" class="select select-sm w-full">
                                <option value="normal">Normal</option>
                                <option value="semibold">Semibold</option>
                                <option value="bold">Bold</option>
                            </select>
                        </fieldset>
                        <fieldset class="fieldset rounded-box">
                            <label class="fieldset-label text-base-content">
                                <input v-model="form.use_filename" type="checkbox" class="checkbox" />
                                Use clip filename
                            </label>
                        </fieldset>
                    </div>
                    <fieldset v-if="form.use_filename" class="fieldset">
                        <legend class="fieldset-legend">Filename regex</legend>
                        <input v-model="form.filename_regex" type="text" class="input input-sm w-full" />
                    </fieldset>

                    <div class="mt-2 grid xs:grid-cols-[auto_150px_150px] gap-4">
                        <div class="grow">
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.xAxis') }}</legend>
                                <input v-model="form.position_x" type="text" name="x" class="input input-sm w-full" />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.yAxis') }}</legend>
                                <input v-model="form.position_y" type="text" name="y" class="input input-sm w-full" />
                            </fieldset>
                        </div>

                        <div class="xs:mt-8.5">
                            <fieldset class="fieldset rounded-box w-full">
                                <label class="fieldset-label text-base-content">
                                    <input v-model="form.background_enabled" type="checkbox" class="checkbox" />
                                    {{ t('message.showBox') }}
                                </label>
                            </fieldset>
                            <fieldset class="fieldset mt-1">
                                <legend class="fieldset-legend">{{ t('message.boxColor') }}</legend>
                                <input v-model="form.background_color" type="color" class="input input-sm w-full cursor-pointer" />
                            </fieldset>
                        </div>
                        <fieldset class="fieldset mt-1 xs:mt-17.5">
                            <legend class="fieldset-legend">{{ t('message.boxAlpha') }}</legend>
                            <input
                                v-model="form.background_opacity"
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
                                    v-model="form.font_size"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.fontColor') }}</legend>
                                <input v-model="form.text_color" type="color" class="input input-sm w-full cursor-pointer" />
                            </fieldset>
                        </div>
                        <div>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.spacing') }}</legend>
                                <input
                                    v-model="form.line_spacing"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.fontAlpha') }}</legend>
                                <input
                                    v-model="form.text_opacity"
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
                                <input
                                    v-model="form.opacity"
                                    type="number"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">{{ t('message.borderWidth') }}</legend>
                                <input
                                    v-model="form.background_padding"
                                    type="number"
                                    min="0"
                                    class="input input-sm w-full"
                                    required
                                />
                            </fieldset>
                        </div>
                    </div>

                    <div class="grid xs:grid-cols-4 gap-4 mt-2">
                        <fieldset class="fieldset">
                            <legend class="fieldset-legend">Scrolling</legend>
                            <select v-model="form.scroll_direction" class="select select-sm w-full">
                                <option value="none">None</option>
                                <option value="left_to_right">Left to right</option>
                                <option value="right_to_left">Right to left</option>
                            </select>
                        </fieldset>
                        <fieldset class="fieldset">
                            <legend class="fieldset-legend">Speed (px/s)</legend>
                            <input v-model="form.scroll_speed" type="number" min="1" class="input input-sm w-full" />
                        </fieldset>
                        <fieldset class="fieldset">
                            <legend class="fieldset-legend">Repeat</legend>
                            <input v-model="form.scroll_repeat" type="number" min="-1" class="input input-sm w-full" />
                            <p class="label">-1 endless, 0 once</p>
                        </fieldset>
                        <div class="grid grid-cols-2 gap-2">
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">Fade in</legend>
                                <input v-model="form.fade_in_seconds" type="number" min="0" step="0.1" class="input input-sm w-full" />
                            </fieldset>
                            <fieldset class="fieldset">
                                <legend class="fieldset-legend">Fade out</legend>
                                <input v-model="form.fade_out_seconds" type="number" min="0" step="0.1" class="input input-sm w-full" />
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
