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
                        :label="(typeof prop === 'boolean') ? '' : name"
                        label-align-sm="right"
                        :label-for="name"
                    >
                        <b-form-checkbox
                            v-if="typeof prop === 'boolean'"
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
                        />
                        <b-form-input
                            v-else-if="typeof prop === 'number'"
                            :id="name"
                            v-model="config[key][name]"
                            type="number"
                            step="0.001"
                        />
                        <b-form-tags
                            v-else-if="Array.isArray(prop)"
                            v-model="config[key][name]"
                            input-id="tags-basic"
                            placeholder="add item..."
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
    </div>
</template>

<script>
export default {
    name: 'Configure',

    components: {},

    async asyncData ({ app, store }) {
        await store.dispatch('auth/inspectToken')
        await store.dispatch('config/getConfig')

        return {
            config: store.state.config.config
        }
    },

    data () {
        return {
        }
    },

    methods: {
        onSubmit (evt) {
            evt.preventDefault()
            this.$store.dispatch('config/setConfig', this.config)
        }
    }
}
</script>

<style lang="scss">
.config-container {
    margin: 2em auto 0;
    padding: 0;
}

.config-group {
    margin-bottom: 2em;
}
</style>
