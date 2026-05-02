<script setup lang="ts">
import { ref } from "vue";
import { api } from "../api/remote";

const emit = defineEmits<{ (e: "login"): void }>();

const password = ref("");
const error = ref("");
const loading = ref(false);

async function handleLogin() {
  error.value = "";
  loading.value = true;
  try {
    const ok = await api.login(password.value);
    if (ok) {
      emit("login");
    } else {
      error.value = "密码错误";
    }
  } catch (e: any) {
    error.value = `连接失败: ${e.message}`;
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <div class="login-container">
    <div class="login-card">
      <h2>TEngine Server</h2>
      <div class="login-field">
        <label>管理密码</label>
        <input v-model="password" type="password" placeholder="输入管理密码" @keyup.enter="handleLogin" />
      </div>
      <div v-if="error" class="login-error">{{ error }}</div>
      <button class="btn btn-primary login-btn" @click="handleLogin" :disabled="loading || !password">
        {{ loading ? "连接中..." : "登录" }}
      </button>
    </div>
  </div>
</template>
