<template>
    <div>
        <Menu />
        <b-container class="messege-container">
            <div class="preset-div">
                <b-row>
                    <b-col>
                        <b-form-select v-model="selected" :options="presets" />
                    </b-col>
                    <b-col cols="2">
                        <b-button-group class="mr-1">
                            <b-button title="Save Preset" variant="primary" @click="savePreset()">
                                <b-icon icon="cloud-upload" />
                            </b-button>
                            <b-button title="New Preset" variant="primary" @click="openDialog()">
                                <b-icon-file-plus />
                            </b-button>
                            <b-button title="Delete Preset" variant="primary" @click="deleteDialog()">
                                <b-icon-file-minus />
                            </b-button>
                        </b-button-group>
                    </b-col>
                </b-row>
            </div>
            <b-form @submit.prevent="submitMessage">
                <b-form-group>
                    <b-form-textarea
                        v-model="form.text"
                        placeholder="Message"
                        rows="7"
                        class="message"
                    />
                </b-form-group>

                <b-row>
                    <b-col>
                        <b-form-group>
                            <b-form-input
                                id="input-1"
                                v-model="form.x"
                                type="text"
                                required
                                placeholder="X"
                            />
                        </b-form-group>

                        <b-form-group>
                            <b-form-input
                                id="input-2"
                                v-model="form.y"
                                type="text"
                                required
                                placeholder="Y"
                            />
                        </b-form-group>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Size"
                                    label-for="input-3"
                                >
                                    <b-form-input
                                        id="input-3"
                                        v-model="form.fontSize"
                                        type="number"
                                        required
                                        value="24"
                                    />
                                </b-form-group>
                            </b-col>
                            <b-col>
                                <b-form-group
                                    label="Spacing"
                                    label-for="input-4"
                                >
                                    <b-form-input
                                        id="input-4"
                                        v-model="form.fontSpacing"
                                        type="number"
                                        required
                                        value="4"
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Font Color"
                                    label-for="input-5"
                                >
                                    <b-form-input
                                        id="input-5"
                                        v-model="form.fontColor"
                                        type="color"
                                        required
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>
                    </b-col>
                    <b-col>
                        <b-form-checkbox
                            v-model="form.showBox"
                            style="margin-bottom: 8px;"
                        >
                            Show Box
                        </b-form-checkbox>

                        <b-row>
                            <b-col>
                                <b-form-group
                                    label="Box Color"
                                    label-for="input-7"
                                >
                                    <b-form-input
                                        id="input-7"
                                        v-model="form.boxColor"
                                        type="color"
                                        required
                                    />
                                </b-form-group>
                            </b-col>
                        </b-row>
                        <b-form-group
                            label="Border Width"
                            label-for="input-9"
                        >
                            <b-form-input
                                id="input-9"
                                v-model="form.border"
                                type="number"
                                required
                                value="4"
                            />
                        </b-form-group>
                    </b-col>
                </b-row>

                <b-form-group
                    label="Overall Alpha"
                    label-for="input-10"
                >
                    <b-form-input
                        id="input-10"
                        v-model="form.overallAlpha"
                        type="text"
                        required
                        value="1"
                    />
                </b-form-group>

                <b-row>
                    <b-col class="sub-btn">
                        <b-button type="submit" class="send-btn" variant="primary">
                            Send
                        </b-button>
                    </b-col>
                    <b-col>
                        <b-alert variant="success" :show="success" dismissible @dismissed="success=false">
                            Sending success...
                        </b-alert>
                        <b-alert variant="warning" :show="failed" dismissible @dismissed="success=failed">
                            Sending failed...
                        </b-alert>
                    </b-col>
                </b-row>
            </b-form>
        </b-container>
        <b-modal
            id="create-modal"
            ref="create-modal"
            title="Create Preset"
            @ok="handleCreate"
        >
            <form ref="form" @submit.stop.prevent="createPreset">
                <b-form-group label="Name" label-for="name-input" invalid-feedback="Name is required">
                    <b-form-input id="name-input" v-model="newPresetName" required />
                </b-form-group>
            </form>
        </b-modal>
        <b-modal
            id="delete-modal"
            ref="delete-modal"
            title="Delete Preset"
            @ok="handleDelete"
        >
            <strong>Delete: "{{ selected }}"?</strong>
        </b-modal>
    </div>
</template>

<script>
import { mapState } from 'vuex'
import Menu from '@/components/Menu.vue'

export default {
    name: 'Media',

    components: {
        Menu
    },

    middleware: 'auth',

    data () {
        return {
            form: {
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
                overallAlpha: 1
            },
            selected: null,
            newPresetName: '',
            presets: [],
            success: false,
            failed: false
        }
    },

    computed: {
        ...mapState('config', ['configID', 'configGui'])
    },

    watch: {
        selected (id) {
            this.getPreset(id)
        }
    },

    created () {
        this.getPreset('')
    },

    methods: {
        async getPreset (id) {
            const response = await this.$axios.get(`api/presets/${this.configGui[this.configID].id}`)

            if (response.data && !id) {
                for (const item of response.data) {
                    this.presets.push({ value: item.id, text: item.name })
                }
            } else if (response.data) {
                this.form = {
                    id: response.data[id].id,
                    name: response.data[id].name,
                    text: response.data[id].text,
                    x: response.data[id].x,
                    y: response.data[id].y,
                    fontSize: response.data[id].font_size,
                    fontSpacing: response.data[id].line_spacing,
                    fontColor: response.data[id].fontcolor,
                    showBox: response.data[id].box,
                    boxColor: response.data[id].boxcolor,
                    border: response.data[id].boxborderw,
                    overallAlpha: response.data[id].alpha
                }
            }
        },
        openDialog () {
            this.$bvModal.show('create-modal')
        },
        handleCreate (bvModalEvt) {
            // Prevent modal from closing
            bvModalEvt.preventDefault()
            // Trigger submit handler
            this.createPreset()
        },
        async createPreset () {
            const preset = {
                name: this.newPresetName,
                message: this.form.text,
                x: this.form.x,
                y: this.form.y,
                font_size: this.form.fontSize,
                font_spacing: this.form.fontSpacing,
                font_color: this.form.fontColor,
                font_alpha: this.form.fontAlpha,
                show_box: this.form.showBox,
                box_color: this.form.boxColor,
                box_alpha: this.form.boxAlpha,
                border_width: this.form.border,
                overall_alpha: this.form.overallAlpha
            }

            const response = await this.$axios.post('api/player/messenger/', preset)

            if (response.status === 201) {
                this.success = true
            } else {
                this.failed = true
            }

            this.$nextTick(() => {
                this.$bvModal.hide('create-modal')
            })
        },
        async savePreset () {
            if (this.selected) {
                const preset = {
                    id: this.form.id,
                    name: this.form.name,
                    message: this.form.text,
                    x: this.form.x,
                    y: this.form.y,
                    font_size: this.form.fontSize,
                    font_spacing: this.form.fontSpacing,
                    font_color: this.form.fontColor,
                    font_alpha: this.form.fontAlpha,
                    show_box: this.form.showBox,
                    box_color: this.form.boxColor,
                    box_alpha: this.form.boxAlpha,
                    border_width: this.form.border,
                    overall_alpha: this.form.overallAlpha
                }

                const response = await this.$axios.put(`api/player/messenger/${this.form.id}/`, preset)

                if (response.status === 200) {
                    this.success = true
                } else {
                    this.failed = true
                }
            }
        },

        deleteDialog () {
            this.$bvModal.show('delete-modal')
        },
        handleDelete (evt) {
            evt.preventDefault()
            this.deletePreset()
        },
        async deletePreset () {
            if (this.selected) {
                await this.$axios.delete(`api/player/messenger/${this.form.id}/`)
            }

            this.$bvModal.hide('delete-modal')
            this.getPreset('')
        },

        async submitMessage () {
            function aToHex (num) {
                return '0x' + Math.round(num * 255).toString(16)
            }

            const obj = {
                text: this.form.text,
                x: this.form.x,
                y: this.form.y,
                fontsize: this.form.fontSize,
                line_spacing: this.form.fontSpacing,
                fontcolor: this.form.fontColor + '@' + aToHex(this.form.fontAlpha),
                alpha: this.form.overallAlpha,
                box: (this.form.showBox) ? 1 : 0,
                boxcolor: this.form.boxColor + '@' + aToHex(this.form.boxAlpha),
                boxborderw: this.form.border
            }

            const response = await this.$axios.post('api/player/send/message/', {
                data: obj,
                channel: this.configGui[this.configID].id
            })

            if (response.data && response.data.status.Success && response.data.status.Success.split(' ')[0] === '0') {
                this.success = true
            } else {
                this.failed = true
            }
        }
    }
}
</script>

<style scoped>
.messege-container {
    margin-top: 5em;
}

.preset-div {
    width: 50%;
    margin-bottom: 2em;
}

.sub-btn {
    min-width: 90px;
    max-width: 100px;
}
</style>
