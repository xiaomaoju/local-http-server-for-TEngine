<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { Plus, Server, Save, Trash2, Eye, EyeOff } from "@lucide/vue";
import Dialog from "./Dialog.vue";
import type { ConnectionCredentials, ConnectionLogin } from "./api";
import { loadConnections, storeConnections, normalizeServerUrl, type ConnectionProfile } from "./connectionProfiles";

const props = defineProps<{ credentials?: ConnectionCredentials; busy: boolean; error: string; notice: string }>();
const emit = defineEmits<{ login: [ConnectionLogin] }>();
const profiles = ref<ConnectionProfile[]>([]);
const selected = ref("");
const name = ref("");
const url = ref(localStorage.getItem("tengine_server_url") || "http://localhost:8083");
const username = ref(localStorage.getItem("tengine_username") || "");
const password = ref("");
const remember = ref(false);
const showPassword = ref(false);
const working = ref(false);
const localError = ref("");
const status = ref("");
const baseline = ref("");
const pending = ref<(() => void) | null>(null);
const deleting = ref(false);
const locked = computed(() => props.busy || working.value);
const draft = () => JSON.stringify([name.value, url.value, username.value, password.value, remember.value]);
const dirty = computed(() => baseline.value !== draft());
const selectedProfile = computed(() => profiles.value.find(p => p.id === selected.value));
function report(e: unknown) { localError.value = e instanceof Error ? e.message : String(e); }
function store(next: ConnectionProfile[]) { storeConnections(next); profiles.value = next; }
async function choose(profile: ConnectionProfile) {
  if (locked.value) return;
  working.value = true; localError.value = ""; status.value = "";
  selected.value = profile.id; name.value = profile.name; url.value = profile.url; username.value = profile.username;
  password.value = ""; remember.value = profile.passwordStored; showPassword.value = false;
  try {
    if (profile.passwordStored) {
      if (!props.credentials) throw new Error("当前环境无法读取系统凭据库，请输入密码。");
      status.value = "正在读取系统凭据库，请查看系统授权提示…";
      password.value = await props.credentials.get(profile.id, profile.url, profile.username) || "";
      status.value = password.value ? "" : "此连接的已存密码不可用，请重新输入后保存。";
    }
  } catch (e) { status.value = ""; report(e); }
  finally { baseline.value = draft(); working.value = false; }
}
function newConnection() {
  selected.value = ""; name.value = ""; url.value = "http://localhost:8083"; username.value = "";
  password.value = ""; remember.value = false; showPassword.value = false; localError.value = ""; status.value = "";
  baseline.value = draft();
}
function switchTo(action: () => void) {
  if (locked.value) return;
  if (dirty.value) pending.value = action; else action();
}
function discard() { const action = pending.value; pending.value = null; action?.(); }
async function save(connect: boolean) {
  if (locked.value) return;
  localError.value = ""; status.value = ""; working.value = true;
  try {
    const endpoint = normalizeServerUrl(url.value);
    const account = username.value.trim();
    if (!name.value.trim()) throw new Error("请输入连接名称，方便区分不同服务器。");
    if (!account) throw new Error("请输入用户名。");
    if ((connect || remember.value) && !password.value) throw new Error("请输入密码。");
    const id = selected.value || crypto.randomUUID();
    if (remember.value) {
      if (!props.credentials) throw new Error("当前环境无法保存密码，请取消“记住密码”后保存连接。");
      status.value = "正在保存到系统凭据库，请完成可能出现的系统提示…";
      await props.credentials.save(id, endpoint, account, password.value);
    } else if (selectedProfile.value?.passwordStored) {
      if (!props.credentials) throw new Error("当前环境无法移除已存密码，请在桌面客户端操作。");
      await props.credentials.remove(id);
    }
    const profile = { id, name: name.value.trim(), url: endpoint, username: account, passwordStored: remember.value, lastUsed: Date.now() };
    store([profile, ...profiles.value.filter(p => p.id !== id)]);
    selected.value = id; url.value = endpoint; username.value = account; name.value = profile.name;
    baseline.value = draft(); status.value = "连接已保存。";
    if (connect) emit("login", {url: endpoint, username: account, password: password.value, connectionId: id});
  } catch (e) { status.value = ""; report(e); }
  finally { working.value = false; }
}
async function remove() {
  if (locked.value || !selectedProfile.value) return;
  working.value = true; localError.value = "";
  try {
    if (selectedProfile.value.passwordStored) {
      if (!props.credentials) throw new Error("当前环境无法移除已存密码，请在桌面客户端操作。");
      await props.credentials.remove(selected.value);
    }
    store(profiles.value.filter(p => p.id !== selected.value));
    deleting.value = false; newConnection(); status.value = "已移除本机连接，服务器和账号不受影响。";
  } catch (e) { deleting.value = false; report(e); }
  finally { working.value = false; }
}
function clearPasswordForChangedTarget() { password.value = ""; }
onMounted(async () => {
  try {
    profiles.value = loadConnections();
    if (profiles.value[0]) await choose(profiles.value[0]);
  } catch (e) { report(e); }
  baseline.value = draft();
});
</script>

<template>
  <section class="ui-connections" aria-label="远程服务器连接">
    <aside class="ui-connection-list" aria-label="已保存的服务器">
      <div class="ui-toolbar"><h2>已保存的服务器</h2><button class="quiet" :disabled="locked" aria-label="新增连接" title="新增连接" @click="switchTo(newConnection)"><Plus :size="16" /></button></div>
      <p v-if="!profiles.length" class="ui-muted">保存连接后，下次可直接选择。</p>
      <button v-for="profile in profiles" :key="profile.id" class="ui-connection-item" :aria-pressed="selected === profile.id" :disabled="locked" @click="switchTo(() => choose(profile))">
        <strong>{{ profile.name }}</strong><span class="ui-mono" :title="profile.url">{{ profile.url }}</span><span v-if="profile.username">{{ profile.username }}</span>
      </button>
    </aside>
    <form class="ui-connection-form ui-form" @submit.prevent="save(true)">
      <h1><Server :size="20" />连接远程服务器</h1>
      <fieldset :disabled="locked" class="ui-form">
        <div class="ui-field"><label for="connection-name">连接名称</label><input id="connection-name" v-model="name" placeholder="例如：群晖测试服" required /></div>
        <div class="ui-field"><label for="server-url">服务地址</label><input id="server-url" v-model="url" type="url" required @input="clearPasswordForChangedTarget" /></div>
        <div class="ui-field"><label for="login-name">用户名</label><input id="login-name" v-model="username" autocomplete="username" required @input="clearPasswordForChangedTarget" /></div>
        <div class="ui-field"><label for="login-password">密码</label><div class="ui-password-input"><input id="login-password" v-model="password" :type="showPassword ? 'text' : 'password'" autocomplete="current-password" /><button type="button" class="quiet" :aria-label="showPassword ? '隐藏密码' : '显示密码'" @click="showPassword = !showPassword"><EyeOff v-if="showPassword" :size="16" /><Eye v-else :size="16" /></button></div></div>
        <label class="ui-remember"><input v-model="remember" type="checkbox" :disabled="!credentials" />记住密码<span class="ui-muted">保存到本机系统凭据库</span></label>
      </fieldset>
      <p v-if="localError || error" class="ui-error" role="alert">{{ localError || error }}</p>
      <p v-if="status || notice" class="ui-notice" role="status">{{ notice || status }}</p>
      <div class="ui-connection-actions">
        <button v-if="selected" type="button" class="ui-action danger" :disabled="locked" @click="deleting = true"><Trash2 :size="16" />移除</button>
        <span class="ui-spacer" />
        <button type="button" :disabled="locked" @click="save(false)"><Save :size="16" />保存连接</button>
        <button type="submit" class="primary" :disabled="locked">{{ busy ? '正在登录…' : '登录' }}</button>
      </div>
      <p class="ui-muted">登录时自动保存连接。账号由管理员创建，忘记密码请联系管理员。</p>
    </form>
  </section>
  <Dialog :open="!!pending" title="连接尚未保存" @close="pending = null"><p>切换连接会放弃当前修改。</p><template #footer><button @click="pending = null">继续编辑</button><button class="danger" @click="discard">放弃修改</button></template></Dialog>
  <Dialog :open="deleting" title="移除已保存的连接" :busy="working" @close="deleting = false"><p>移除“{{ name }}”的本机连接及记住的密码？服务器和账号不会删除。</p><template #footer><button :disabled="working" @click="deleting = false">取消</button><button class="danger" :disabled="working" @click="remove">移除连接</button></template></Dialog>
</template>
