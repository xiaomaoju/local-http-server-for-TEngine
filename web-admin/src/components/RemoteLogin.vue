<script setup lang="ts">
import { ref, onMounted } from "vue";
import { api } from "../api/remote";

const emit = defineEmits<{ (e: "login"): void }>();

const STORAGE_KEY = "tengine_web_login";

const password = ref("");
const rememberPassword = ref(true);
const error = ref("");
const loading = ref(false);
const autoTried = ref(false);

onMounted(async () => {
  try {
    const data = localStorage.getItem(STORAGE_KEY);
    if (data) {
      const obj = JSON.parse(data);
      if (obj.password) {
        password.value = obj.password;
        rememberPassword.value = true;
        // Try auto-login if password was saved
        autoTried.value = true;
        loading.value = true;
        try {
          const ok = await api.login(password.value);
          if (ok) {
            emit("login");
            return;
          }
        } catch {}
        loading.value = false;
      }
    }
  } catch {}
});

async function handleLogin() {
  error.value = "";
  loading.value = true;
  try {
    const ok = await api.login(password.value);
    if (ok) {
      try {
        localStorage.setItem(
          STORAGE_KEY,
          JSON.stringify({
            password: rememberPassword.value ? password.value : "",
          }),
        );
      } catch {}
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
  <div class="login-wrap">
    <div class="login-card">
      <div class="login-brand">
        <div class="login-logo">⚡</div>
        <h2>TEngine Server</h2>
        <p class="login-subtitle">远程资源管理</p>
      </div>

      <div class="login-form">
        <div class="login-field">
          <label>管理密码</label>
          <input
            v-model="password"
            type="password"
            placeholder="输入管理密码"
            @keyup.enter="handleLogin"
            :disabled="loading"
            autofocus
          />
        </div>

        <label class="login-checkbox">
          <input type="checkbox" v-model="rememberPassword" />
          <span>记住密码（保存在浏览器本地）</span>
        </label>

        <div v-if="error" class="login-error">{{ error }}</div>

        <button
          class="login-submit"
          @click="handleLogin"
          :disabled="loading || !password"
        >
          {{ loading ? "连接中..." : "登 录" }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.login-wrap {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: radial-gradient(circle at 30% 20%, rgba(34, 211, 238, 0.08), transparent 50%),
              radial-gradient(circle at 70% 80%, rgba(74, 222, 128, 0.06), transparent 50%);
}
.login-card {
  background: var(--bg-secondary);
  border: 1px solid var(--border);
  border-radius: 16px;
  padding: 36px 40px;
  width: 380px;
  max-width: 100%;
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.4);
}
.login-brand {
  text-align: center;
  margin-bottom: 28px;
}
.login-logo {
  font-size: 36px;
  margin-bottom: 8px;
}
.login-card h2 {
  margin: 0 0 4px;
  color: var(--accent);
  font-size: 20px;
  font-weight: 600;
  letter-spacing: 0.5px;
}
.login-subtitle {
  margin: 0;
  font-size: 12px;
  color: var(--text-muted);
}
.login-form {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.login-field label {
  display: block;
  color: var(--text-secondary);
  font-size: 12px;
  margin-bottom: 6px;
  font-weight: 500;
}
.login-field input {
  width: 100%;
  height: 40px;
  padding: 0 14px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 8px;
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s, box-shadow 0.15s, background 0.15s;
  font-family: inherit;
  box-sizing: border-box;
}
.login-field input::placeholder {
  color: var(--text-muted);
}
.login-field input:focus {
  border-color: var(--accent);
  background: var(--bg-secondary);
  box-shadow: 0 0 0 3px rgba(34, 211, 238, 0.15);
}
.login-checkbox {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}
.login-checkbox input {
  width: 14px;
  height: 14px;
  accent-color: var(--accent);
  cursor: pointer;
}
.login-error {
  color: #ff6b6b;
  font-size: 12px;
  background: rgba(255, 107, 107, 0.08);
  border: 1px solid rgba(255, 107, 107, 0.25);
  padding: 8px 12px;
  border-radius: 6px;
}
.login-submit {
  height: 44px;
  border: none;
  border-radius: 8px;
  background: var(--accent);
  color: var(--bg-primary);
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: transform 0.1s, box-shadow 0.15s, opacity 0.15s;
  margin-top: 4px;
  letter-spacing: 2px;
}
.login-submit:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 6px 20px rgba(34, 211, 238, 0.3);
}
.login-submit:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
