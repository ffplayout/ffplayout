<template>
    <div>
        <div class="flex flex-col items-center pt-10 px-8">
            <div class="mt-2 w-full max-w-4xl">
                <div class="flex flex-col xs:flex-row w-full gap-4">
                    <div class="grow xs:max-w-72">
                        <select
                            v-model="selected"
                            class="select select-sm select-bordered w-full"
                            @change="onChange($event)"
                        >
                            <option v-for="item in presets" :key="item.name">{{ item.name }}</option>
                        </select>
                    </div>
                    <div class="join">
                        <button
                            class="btn btn-sm join-item btn-primary"
                            :title="$t('message.savePreset')"
                            @click="savePreset()"
                        >
                            <i class="bi-cloud-upload" />
                        </button>
                        <button
                            class="btn btn-sm join-item btn-primary"
                            :title="$t('message.newPreset')"
                            @click="showCreateModal = true"
                        >
                            <i class="bi-file-plus" />
                        </button>
                        <button
                            class="btn btn-sm join-item btn-primary"
                            :title="$t('message.delPreset')"
                            @click="showDeleteModal = true"
                        >
                            <i class="bi-file-minus" />
                        </button>
                    </div>
                </div>

                <form class="my-6 w-full" @submit.prevent="submitMessage">
                    <textarea
                        v-model="form.text"
                        class="textarea textarea-bordered w-full"
                        rows="4"
                        :placeholder="$t('message.placeholder')"
                    />

                    <div class="mt-2 grid xs:grid-cols-[auto_150px_150px] gap-4">
                        <div class="grow">
                            <div class="form-control">
                                <label class="cursor-pointer p-0">
                                    <div class="label">
                                        <span class="label-text">{{ $t('message.xAxis') }}</span>
                                    </div>
                                    <input
                                        v-model="form.x"
                                        class="input input-sm input-bordered w-full"
                                        type="text"
                                        placeholder="X"
                                        required
                                    />
                                </label>
                            </div>

                            <div class="form-control">
                                <label class="cursor-pointer p-0">
                                    <div class="label">
                                        <span class="label-text">{{ $t('message.yAxis') }}</span>
                                    </div>
                                    <input
                                        v-model="form.y"
                                        class="input input-sm input-bordered w-full"
                                        type="text"
                                        placeholder="Y"
                                        required
                                    />
                                </label>
                            </div>
                        </div>

                        <div class="xs:mt-10">
                            <div class="form-control">
                                <label class="label cursor-pointer p-0">
                                    <span class="label-text">{{ $t('message.showBox') }}</span>
                                    <input
                                        v-model="form.showBox"
                                        type="checkbox"
                                        class="checkbox checkbox-xs rounded-sm"
                                    />
                                </label>
                            </div>

                            <label class="mt-2 form-control w-full">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.boxColor') }}</span>
                                </div>
                                <input
                                    v-model="form.boxColor"
                                    type="color"
                                    class="input input-sm input-bordered w-full p-1"
                                    required
                                />
                            </label>
                        </div>
                        <label class="form-control w-full xs:mt-[68px]">
                            <div class="label">
                                <span class="label-text">{{ $t('message.boxAlpha') }}</span>
                            </div>
                            <input
                                v-model="form.boxAlpha"
                                type="number"
                                min="0"
                                max="1"
                                step="0.01"
                                class="input input-sm input-bordered w-full"
                                required
                            />
                        </label>
                    </div>
                    <div class="grid xs:grid-cols-[150px_150px_auto] gap-4 mt-2">
                        <div>
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.size') }}</span>
                                </div>
                                <input
                                    v-model="form.fontSize"
                                    type="number"
                                    class="input input-sm input-bordered w-full"
                                    required
                                />
                            </label>

                            <label class="form-control w-full mt-2">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.fontColor') }}</span>
                                </div>
                                <input
                                    v-model="form.fontColor"
                                    type="color"
                                    class="input input-sm input-bordered w-full p-1"
                                    required
                                />
                            </label>
                        </div>
                        <div>
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.spacing') }}</span>
                                </div>
                                <input
                                    v-model="form.fontSpacing"
                                    type="number"
                                    class="input input-sm input-bordered w-full"
                                    required
                                />
                            </label>
                            <label class="form-control w-full mt-2">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.fontAlpha') }}</span>
                                </div>
                                <input
                                    v-model="form.fontAlpha"
                                    type="number"
                                    class="input input-sm input-bordered w-full"
                                    min="0"
                                    max="1"
                                    step="0.01"
                                    required
                                />
                            </label>
                        </div>

                        <div class="grow">
                            <label class="form-control w-full">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.overallAlpha') }}</span>
                                </div>
                                <input
                                    v-model="form.overallAlpha"
                                    type="text"
                                    class="input input-sm input-bordered w-full"
                                    required
                                />
                            </label>
                            <label class="form-control w-full xs:max-w-[150px] mt-2">
                                <div class="label">
                                    <span class="label-text">{{ $t('message.borderWidth') }}</span>
                                </div>
                                <input
                                    v-model="form.border"
                                    type="number"
                                    class="input input-sm input-bordered w-full"
                                    required
                                />
                            </label>
                        </div>
                    </div>

                    <div class="mt-5">
                        <button class="btn btn-primary send-btn" type="submit">{{ $t('message.send') }}</button>
                    </div>
                </form>
            </div>
        </div>

        <GenericModal :show="showCreateModal" :title="$t('message.newPreset')" :modal-action="createNewPreset">
            <label class="form-control w-full">
                <div class="label">
                    <span class="label-text">{{ $t('message.name') }}</span>
                </div>
                <input v-model="newPresetName" type="text" class="input input-bordered w-full" />
            </label>
        </GenericModal>

        <GenericModal
            :show="showDeleteModal"
            :title="$t('message.delPreset')"
            :text="`${$t('message.delText')}: <strong> ${selected}</strong>?`"
            :modal-action="deletePreset"
        />
    </div>
</template>

<script setup lang="ts">
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { numberToHex, hexToNumber } = stringFormatter()

useHead({
    title: 'Messages | ffplayout',
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

async function getPreset(index: number) {
    fetch(`/api/presets/${configStore.configGui[configStore.configID].id}`, {
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
            channel_id: configStore.configGui[configStore.configID].id,
        }

        const response = await fetch(`/api/presets/${form.value.id}`, {
            method: 'PUT',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify(preset),
        })

        if (response.status === 200) {
            indexStore.msgAlert('success', 'Save Preset done!', 2)
        } else {
            indexStore.msgAlert('error', 'Save Preset failed!', 2)
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
            channel_id: configStore.configGui[configStore.configID].id,
        }

        const response = await fetch('/api/presets/', {
            method: 'POST',
            headers: { ...configStore.contentType, ...authStore.authHeader },
            body: JSON.stringify(preset),
        })

        if (response.status === 200) {
            indexStore.msgAlert('success', 'Save Preset done!', 2)
            getPreset(-1)
        } else {
            indexStore.msgAlert('error', 'Save Preset failed!', 2)
        }
    }

    newPresetName.value = ''
}

async function deletePreset(del: boolean) {
    showDeleteModal.value = false

    if (del && selected.value && selected.value !== '') {
        await fetch(`/api/presets/${form.value.id}`, {
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

    const response = await fetch(`/api/control/${configStore.configGui[configStore.configID].id}/text/`, {
        method: 'POST',
        headers: { ...configStore.contentType, ...authStore.authHeader },
        body: JSON.stringify(obj),
    })

    if (response.status === 200) {
        indexStore.msgAlert('success', 'Sending success...', 2)
    } else {
        indexStore.msgAlert('error', 'Sending failed...', 2)
    }
}
</script>
