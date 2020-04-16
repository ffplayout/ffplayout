<template>
    <div>
        <div class="menu">
            <b-button-group size="sm">
                <b-button to="/" variant="primary">
                    Home
                </b-button>
                <b-button to="control" variant="primary">
                    Control
                </b-button>
                <b-button to="media" variant="primary">
                    Media
                </b-button>
                <b-button to="logging" variant="primary">
                    Logging
                </b-button>
                <b-button to="configure" variant="primary">
                    Configure
                </b-button>
                <b-button to="/" variant="primary" @click="logout()">
                    Logout
                </b-button>
            </b-button-group>
        </div>
    </div>
</template>

<script>
export default {
    name: 'Menu',

    computed: {
    },

    methods: {
        async logout () {
            try {
                await this.$store.commit('auth/REMOVE_TOKEN')
                await this.$store.commit('auth/UPDATE_IS_LOGIN', false)
            } catch (e) {
                this.formError = e.message
            }
        }
    }
}
</script>

<style lang="scss" >
.menu {
    width: 100%;
    height: 40px;
    margin: 0;
    padding: .5em;
}

.menu div {
    float: right;
}
</style>
