<script setup lang="ts">
import Workspace from "../../shared/Workspace.vue";
import { api } from "./api/remote";
</script>
<template><Workspace :api="api" /></template>
