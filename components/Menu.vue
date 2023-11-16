<template>
    <div>
        <div class="menu">
            <nav class="navbar navbar-expand-sm fixed-top custom-nav">
                <div class="container-fluid">
                    <NuxtLink class="navbar-brand p-2" href="/"
                        ><img
                            src="~/assets/images/ffplayout-small.png"
                            class="img-fluid"
                            alt="Logo"
                            width="30"
                            height="30"
                    /></NuxtLink>
                    <button
                        class="navbar-toggler"
                        type="button"
                        data-bs-toggle="collapse"
                        data-bs-target="#navbarNavDropdown"
                        aria-controls="navbarNavDropdown"
                        aria-expanded="false"
                        aria-label="Toggle navigation"
                    >
                        <span class="navbar-toggler-icon"> </span>
                    </button>
                    <div class="collapse navbar-collapse justify-content-end" id="navbarNavDropdown">
                        <ul class="navbar-nav">
                            <li class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/">Home</NuxtLink>
                            </li>
                            <li class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/player">Player</NuxtLink>
                            </li>
                            <li class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/media">Media</NuxtLink>
                            </li>
                            <li class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/message">Message</NuxtLink>
                            </li>
                            <li class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/logging">Logging</NuxtLink>
                            </li>
                            <li v-if="authStore.role.toLowerCase() == 'admin'" class="nav-item">
                                <NuxtLink class="btn btn-primary btn-sm" to="/configure">Configure</NuxtLink>
                            </li>
                            <li v-if="configStore.configGui.length > 1" class="nav-item dropdown">
                                &nbsp;
                                <a
                                    class="btn btn-primary btn-sm dropdown-toggle"
                                    href="#"
                                    role="button"
                                    data-bs-toggle="dropdown"
                                    aria-expanded="false"
                                >
                                    {{ configStore.configGui[configStore.configID].name }}
                                </a>
                                <ul class="dropdown-menu dropdown-menu-dark dropdown-menu-end">
                                    <li v-for="(channel, index) in configStore.configGui" :key="index">
                                        <a class="dropdown-item" @click="selectChannel(index)">{{ channel.name }}</a>
                                    </li>
                                </ul>
                                &nbsp;
                            </li>
                            <li class="nav-item">
                                <a class="btn btn-primary btn-sm" @click="logout()">Logout</a>
                            </li>
                        </ul>
                    </div>
                </div>
            </nav>
        </div>
    </div>
</template>

<script setup lang="ts">
import { useAuth } from '~/stores/auth'
import { useConfig } from '~/stores/config'

const authStore = useAuth()
const configStore = useConfig()
const router = useRouter()

function logout() {
    authStore.removeToken()
    authStore.isLogin = false
    router.push({ path: '/' })
}

function selectChannel(index: number) {
    configStore.configID = index
    configStore.getPlayoutConfig()
}
</script>

<style lang="scss">
.menu {
    width: 100%;
    height: 60px;
    margin: 0;

    div {
        padding: 0.3em;
    }
}

.custom-nav {
    background-color: $bg-primary;
}

.nav-item .btn {
    position: relative;
}

.router-link-exact-active::after {
    background: $accent;
    content: ' ';
    width: 100%;
    height: 2px;
    position: absolute;
    display: block;
    left: 0;
    right: 0;
    border-radius: 1px;
}

@media (max-width: 575px) {
    .nav-item .btn {
        width: 100%;
        text-align: left;
        margin-bottom: .3em;
    }
}
</style>
