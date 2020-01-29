<template>
    <b-container class="login-container">
        <div>
            <div class="header">
                <h1>ffplayout</h1>
            </div>

            <div v-if="!$store.state.auth.isLogin">
                <b-form @submit.prevent="login" class="login-form">
                    <p v-if="formError" class="error">
                        {{ formError }}
                    </p>
                    <b-form-group id="input-group-1" label="User:" label-for="input-user">
                        <b-form-input id="input-user" v-model="formUsername" type="text" required placeholder="Username" />
                    </b-form-group>
                    <b-form-group id="input-group-1" label="Password:" label-for="input-pass">
                        <b-form-input id="input-pass" v-model="formPassword" type="password" required placeholder="Password" />
                    </b-form-group>
                    <b-button type="submit" variant="primary">
                        Login
                    </b-button>
                </b-form>
            </div>
            <div v-else>
                <br>
                <br>
                <h3>Wellcome to ffplayout manager!</h3>
            </div>
        </div>
    </b-container>
</template>

<script>
export default {
    components: {},

    data () {
        return {
            formError: null,
            formUsername: '',
            formPassword: ''
        }
    },
    created () {
        this.init()
    },
    methods: {
        async init () {
            await this.$store.dispatch('auth/inspectToken')
            this.checkLogin()
        },
        async login () {
            try {
                await this.$store.dispatch('auth/obtainToken', {
                    username: this.formUsername,
                    password: this.formPassword
                })
                this.formUsername = ''
                this.formPassword = ''
                this.formError = null

                this.checkLogin()
            } catch (e) {
                this.formError = e.message
            }
        },
        async logout () {
            try {
                await this.$store.commit('auth/REMOVE_TOKEN')
                await this.$store.commit('auth/UPDATE_IS_LOGIN', false)
            } catch (e) {
                this.formError = e.message
            }
        },
        checkLogin () {
            if (this.$store.state.auth.isLogin) {
                // this.$router.push('/player')
            }
        }
    }
}
</script>

<style>
.login-container {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
}

.header {
    text-align: center;
    margin-bottom: 3em;
}

.login-form {
    min-width: 300px;
}
</style>
