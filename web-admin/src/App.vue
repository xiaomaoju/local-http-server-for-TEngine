<script setup lang="ts">
import { ref } from "vue";
import RemoteLogin from "./components/RemoteLogin.vue";
import ProjectManager from "./components/ProjectManager.vue";

const loggedIn = ref(false);
</script>

<template>
  <RemoteLogin v-if="!loggedIn" @login="loggedIn = true" />
  <ProjectManager v-else @logout="loggedIn = false" />
</template>
