<template>
    <div>
        <div class="menu">
            <b-nav align="right">
                <b-nav-item to="/" class="nav-item" exact-active-class="active-menu-item">
                    Home
                </b-nav-item>
                <b-nav-item to="/player" exact-active-class="active-menu-item">
                    Player
                </b-nav-item>
                <b-nav-item to="/media" exact-active-class="active-menu-item">
                    Media
                </b-nav-item>
                <b-nav-item to="/message" exact-active-class="active-menu-item">
                    Message
                </b-nav-item>
                <b-nav-item to="/logging" exact-active-class="active-menu-item">
                    Logging
                </b-nav-item>
                <b-nav-item to="/configure" exact-active-class="active-menu-item">
                    Configure
                </b-nav-item>
                <b-nav-item-dropdown :text="configGui[configID].channel" right>
                    <b-dropdown-item v-for="(channel, index) in configGui" :key="channel.key" @click="selectChannel(index)">
                        {{ channel.channel }}
                    </b-dropdown-item>
                </b-nav-item-dropdown>
                <b-nav-item to="/" @click="logout()">
                    Logout
                </b-nav-item>
            </b-nav>
        </div>
    </div>
</template>

<script>
import { mapState } from 'vuex'

export default {
    name: 'Menu',

    computed: {
        ...mapState('config', ['configID', 'configGui'])
    },

    methods: {
        async logout () {
            try {
                await this.$store.commit('auth/REMOVE_TOKEN')
                await this.$store.commit('auth/UPDATE_IS_LOGIN', false)
            } catch (e) {
                this.formError = e.message
            }
        },

        selectChannel (index) {
            this.$store.commit('config/UPDATE_CONFIG_ID', index)
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

.nav-item {
    background-image: linear-gradient(#484e55, #3A3F44 60%, #313539);
    background-repeat: no-repeat;
    height: 28px;
    margin: .05em;
    border-radius: 3px;
    font-size: .95em;
}

.nav-item:hover {
    background-image: linear-gradient(#5a636c, #4c545b 60%, #42484e);
    background-repeat: no-repeat;
}

.nav-item a {
    padding: .2em .6em .2em .6em;
}

.active-menu-item {
    position: relative;
}

.active-menu-item::after {
    background: #ff9c36;
    content: " ";
    width: 100%;
    height: 2px;
    color: red;
    position: absolute;
    display: block;
    left: 0;
    right: 0;
    border-radius: 1px;
}
</style>
