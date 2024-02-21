<template>
    <div>
        <Menu />
        <div class="container-fluid configure-container">
            <div class="d-flex align-items-start h-100">
                <div class="nav flex-column nav-pills h-100 me-3" id="v-pills-tab" role="tablist" aria-orientation="vertical">
                    <button
                        class="nav-link active"
                        id="v-pills-gui-tab"
                        data-bs-toggle="pill"
                        data-bs-target="#v-pills-gui"
                        type="button"
                        role="tab"
                        aria-controls="v-pills-gui"
                        aria-selected="true"
                    >
                        GUI
                    </button>
                    <button
                        class="nav-link"
                        id="v-pills-playout-tab"
                        data-bs-toggle="pill"
                        data-bs-target="#v-pills-playout"
                        type="button"
                        role="tab"
                        aria-controls="v-pills-playout"
                        aria-selected="false"
                    >
                        Playout
                    </button>
                    <button
                        class="nav-link"
                        id="v-pills-user-tab"
                        data-bs-toggle="pill"
                        data-bs-target="#v-pills-user"
                        type="button"
                        role="tab"
                        aria-controls="v-pills-user"
                        aria-selected="false"
                    >
                        User
                    </button>
                </div>
                <div class="tab-content h-100" id="v-pills-tabContent">
                    <div
                        class="tab-pane h-100 overflow-y-auto show active"
                        id="v-pills-gui"
                        role="tabpanel"
                        aria-labelledby="v-pills-gui-tab"
                    >
                        <div class="config-container">
                            <GuiConfig />
                        </div>
                    </div>
                    <div class="tab-pane h-100 overflow-y-auto" id="v-pills-playout" role="tabpanel" aria-labelledby="v-pills-playout-tab">
                        <div class="config-container">
                            <PlayoutConfig />
                        </div>
                    </div>
                    <div class="tab-pane h-100 overflow-y-auto" id="v-pills-user" role="tabpanel" aria-labelledby="v-pills-user-tab">
                        <div class="config-container">
                            <UserConfig />
                        </div>
                    </div>
                </div>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
useHead({
    title: 'Configuration | ffplayout'
})

const indexStore = useIndex()
</script>

<style lang="scss">
.configure-container {
    height: calc(100% - 90px);
}

.config-container {
    margin: 2em auto 2em auto;
    padding: 0;
    width: 90vw;
}
</style>
