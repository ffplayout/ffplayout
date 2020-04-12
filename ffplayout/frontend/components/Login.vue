<template>
    <div>
        <b-modal
            id="modal-prevent-closing"
            ref="my-modal"
            title="Submit Your Name"
            @show="resetModal"
            @hidden="resetModal"
            @ok="handleOk"
        >
            <form ref="form" @submit.stop.prevent="handleSubmit">
                <b-form-group :state="nameState" label="Name" label-for="name-input" invalid-feedback="Name is required">
                    <b-form-input id="name-input" v-model="name" :state="nameState" required />
                </b-form-group>
                <b-form-group :state="passState" label="Password" label-for="pass-input" invalid-feedback="Password is required">
                    <b-form-input id="pass-input" v-model="pass" :state="passState" type="password" required />
                </b-form-group>
            </form>
        </b-modal>
    </div>
</template>

<script>
import { mapState } from 'vuex'

export default {
    name: 'Login',

    props: {
        show: {
            type: Boolean,
            default: false
        }
    },

    data () {
        return {
            formError: null,
            name: '',
            pass: '',
            nameState: null,
            passState: null
        }
    },

    computed: {
        ...mapState('auth', ['isLogin'])
    },

    mounted () {
        if (this.show) {
            this.$refs['my-modal'].show()
        }
    },

    methods: {
        checkFormValidity () {
            const valid = this.$refs.form.checkValidity()
            this.nameState = valid
            return valid
        },
        resetModal () {
            this.name = ''
            this.pass = ''
            this.nameState = null
            this.passState = null
        },
        handleOk (bvModalEvt) {
            // Prevent modal from closing
            bvModalEvt.preventDefault()
            // Trigger submit handler
            this.handleSubmit()
        },
        async handleSubmit () {
            // Exit when the form isn't valid
            if (!this.checkFormValidity()) {
                return
            }

            try {
                await this.$store.dispatch('auth/obtainToken', {
                    username: this.name,
                    password: this.pass
                })
                this.formError = null

                window.location.reload(true)
            } catch (e) {
                this.formError = e.message
            }

            // Hide the modal manually
            this.$nextTick(() => {
                this.$bvModal.hide('modal-prevent-closing')
            })
        }
    }
}
</script>
