<template>
    <div>
        <b-container class="config-container">
            <b-form v-if="config" @submit="onSubmit">
                <b-form-group
                    v-for="(item, key, index) in config"
                    :key="index"
                    label-cols-lg="2"
                    :label="key"
                    label-size="lg"
                    label-class="font-weight-bold pt-0"
                    class="config-group"
                >
                    <b-form-group
                        v-for="(prop, name, idx) in item"
                        :key="idx"
                        label-cols-sm="2"
                        :label="(typeof prop === 'boolean' || name === 'helptext') ? '' : name"
                        label-align-sm="right"
                        :label-for="name"
                    >
                        <b-form-textarea
                            v-if="name === 'helptext'"
                            id="textarea-plaintext"
                            plaintext
                            :value="prop"
                            rows="2"
                            max-rows="8"
                            class="text-area"
                        />
                        <b-form-checkbox
                            v-else-if="typeof prop === 'boolean'"
                            :id="name"
                            v-model="config[key][name]"
                            :name="name"
                        >
                            {{ name }}
                        </b-form-checkbox>
                        <b-form-input
                            v-else-if="typeof prop === 'number' && Number.isInteger(prop)"
                            :id="name"
                            v-model="config[key][name]"
                            type="number"
                            class="input-field"
                        />
                        <b-form-input
                            v-else-if="typeof prop === 'number'"
                            :id="name"
                            v-model="config[key][name]"
                            type="number"
                            step="0.001"
                            class="input-field"
                        />
                        <b-form-tags
                            v-else-if="Array.isArray(prop)"
                            v-model="config[key][name]"
                            input-id="tags-basic"
                            separator=" ,;"
                            :placeholder="`add ${name}...`"
                            class="mb-2"
                        />
                        <b-form-input v-else :id="name" v-model="config[key][name]" :value="prop" />
                    </b-form-group>
                </b-form-group>

                <b-button type="submit" variant="primary">
                    Save
                </b-button>
            </b-form>
        </b-container>
        <Login :show="showLogin" />
    </div>
</template>

<script>
import Login from '@/components/Login.vue'

export default {
    name: 'Configure',

    components: {
        Login
    },

    async asyncData ({ app, store }) {
        await store.dispatch('auth/inspectToken')
        let login = false

        if (store.state.auth.isLogin) {
            await store.dispatch('config/getConfig')
        } else {
            login = true
        }

        return {
            config: store.state.config.config,
            showLogin: login
        }
    },

    data () {
        return {
        }
    },

    methods: {
        async onSubmit (evt) {
            evt.preventDefault()
            await this.$store.dispatch('auth/inspectToken')
            this.$store.dispatch('config/setConfig', this.config)
        }
    }
}
</script>

<style lang="scss">
.config-container {
    margin: 2em auto 2em auto;
    padding: 0;
}

.config-group {
    margin-bottom: 2em;
}

.input-field {
    max-width: 200px;
}

.text-area {
    overflow-y: hidden !important;
}
</style>
