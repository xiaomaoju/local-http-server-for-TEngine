<script setup lang="ts">
import { ref, onMounted, computed } from "vue";
import { Plus, Search, UserRound, Trash2 } from "@lucide/vue";
import Dialog from "./Dialog.vue";
import StatusBadge from "./StatusBadge.vue";
import ActionButton from "./ActionButton.vue";
import { type RemoteApi, type User, type ProjectConfig } from "./api";
const props = defineProps<{
  api: RemoteApi;
  projects: ProjectConfig[];
  currentUser: User;
}>();
const emit = defineEmits<{ changed: [] }>();
const users = ref<User[]>([]);
const loading = ref(false);
const error = ref("");
const query = ref("");
const mode = ref<"" | "create" | "edit" | "password" | "delete">("");
const target = ref<User>();
const username = ref("");
const password = ref("");
const role = ref("user");
const enabled = ref(true);
const transfers = ref<Record<string, string>>({});
const receiver = ref("");
const discardOpen = ref(false);
let draftBaseline = "";
function draft() {
  return JSON.stringify([
    username.value,
    password.value,
    role.value,
    enabled.value,
    receiver.value,
    transfers.value,
  ]);
}
function close() {
  mode.value = "";
  password.value = "";
  discardOpen.value = false;
}
function requestClose() {
  if (loading.value) return;
  if (draft() !== draftBaseline) discardOpen.value = true;
  else close();
}
const displayed = computed(() =>
  users.value.filter((u) =>
    u.username.toLowerCase().includes(query.value.toLowerCase()),
  ),
);
const owned = computed(() =>
  props.projects.filter((p) => p.owner_id === target.value?.id),
);
const recipients = computed(() =>
  users.value.filter((u) => u.enabled && u.id !== target.value?.id),
);
async function load() {
  loading.value = true;
  error.value = "";
  try {
    users.value = await props.api.request<User[]>("/api/admin/users");
  } catch (e) {
    error.value = String(e);
  } finally {
    loading.value = false;
  }
}
onMounted(load);
function open(next: typeof mode.value, user?: User) {
  target.value = user;
  mode.value = next;
  username.value = user?.username || "";
  password.value = "";
  role.value = user?.role || "user";
  enabled.value = user?.enabled ?? true;
  error.value = "";
  transfers.value = {};
  receiver.value = "";
  draftBaseline = draft();
}
async function submit() {
  if (loading.value) return;
  loading.value = true;
  error.value = "";
  try {
    if (mode.value === "create")
      await props.api.mutate("/api/admin/users", "POST", {
        username: username.value,
        password: password.value,
        role: role.value,
      });
    if (mode.value === "edit")
      await props.api.mutate(`/api/admin/users/${target.value!.id}`, "PUT", {
        role: role.value,
        enabled: enabled.value,
      });
    if (mode.value === "password")
      await props.api.mutate(
        `/api/admin/users/${target.value!.id}/password`,
        "PUT",
        { password: password.value },
      );
    if (mode.value === "delete") {
      if (owned.value.some((p) => !transfers.value[p.id])) {
        error.value = "请为每个项目指定接收者。";
        return;
      }
      await props.api.mutate(`/api/admin/users/${target.value!.id}`, "DELETE", {
        transfers: transfers.value,
      });
    }
    mode.value = "";
    password.value = "";
    await load();
    emit("changed");
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}
function assignAll() {
  for (const p of owned.value) transfers.value[p.id] = receiver.value;
}
</script>
<template>
  <div class="ui-toolbar">
    <Search :size="16" /><input
      v-model="query"
      class="ui-search"
      aria-label="搜索用户"
      placeholder="搜索用户名"
    /><span class="ui-spacer" /><button class="primary" @click="open('create')">
      <Plus :size="16" />创建用户
    </button>
  </div>
  <p class="ui-muted">
    管理员创建账号；用户首次登录需修改密码。停用账号会立即撤销登录会话。
  </p>
  <div v-if="error && !mode" class="ui-error" role="alert">
    {{ error }}<button @click="load">重试</button>
  </div>
  <div class="ui-table-wrap">
    <table class="ui-table">
      <thead>
        <tr>
          <th>用户</th>
          <th>身份</th>
          <th>状态</th>
          <th>操作</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="u in displayed" :key="u.id">
          <td>
            <span class="ui-inline"
              ><UserRound :size="14" />{{ u.username
              }}<span v-if="u.id === currentUser.id" class="ui-muted"
                >你</span
              ></span
            >
          </td>
          <td>{{ u.role === "admin" ? "管理员" : "普通用户" }}</td>
          <td><StatusBadge :tone="!u.enabled ? 'danger' : u.must_change_password ? 'warning' : 'success'">{{ !u.enabled ? '已停用' : u.must_change_password ? '首次登录待改密' : '正常' }}</StatusBadge></td>
          <td>
            <div class="ui-inline">
              <button class="ui-action" @click="open('edit', u)">编辑</button
              ><button class="ui-action" @click="open('password', u)">
                重置密码</button
              ><ActionButton label="删除" danger :disabled="u.id === currentUser.id" reason="请由另一位管理员删除当前账号。" @click="open('delete', u)"><template #icon><Trash2 :size="14" /></template></ActionButton>
            </div>
          </td>
        </tr>
        <tr v-if="!displayed.length">
          <td colspan="4" class="ui-muted">
            {{ loading ? "正在读取用户…" : "没有匹配的用户" }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
  <Dialog
    :open="!!mode"
    :title="
      mode === 'create'
        ? '创建用户'
        : mode === 'delete'
          ? '转交项目并删除用户'
          : mode === 'password'
            ? '重置密码'
            : '编辑用户'
    "
    :busy="loading"
    @close="requestClose"
  >
    <form id="user-form" class="ui-form" @submit.prevent="submit">
      <p v-if="target">
        账号：<strong>{{ target.username }}</strong>
      </p>
      <div v-if="mode === 'create'" class="ui-field">
        <label for="new-username">用户名</label
        ><input
          id="new-username"
          v-model="username"
          required
          minlength="3"
          maxlength="40"
          pattern="[A-Za-z0-9_.\-]+"
          autocomplete="off"
        /><small class="ui-muted">3–40 位英文字母、数字、下划线或短横线</small>
      </div>
      <div v-if="mode === 'create' || mode === 'password'" class="ui-field">
        <label for="initial-password">初始密码</label
        ><input
          id="initial-password"
          v-model="password"
          type="password"
          required
          minlength="10"
          maxlength="256"
          autocomplete="new-password"
        /><small class="ui-muted"
          >至少 10 个字符；请通过可信方式告知用户，登录后必须修改。</small
        >
      </div>
      <div v-if="mode === 'create' || mode === 'edit'" class="ui-field">
        <label for="user-role">系统身份</label
        ><select id="user-role" v-model="role">
          <option value="user">普通用户</option>
          <option value="admin">管理员</option>
        </select>
      </div>
      <label v-if="mode === 'edit'" class="ui-inline"
        ><input v-model="enabled" type="checkbox" />启用账号</label
      >
      <template v-if="mode === 'delete'">
        <p>账号及共享关系将删除，项目资源保留。此操作不可撤销。</p>
        <template v-if="owned.length"
          ><div class="ui-field">
            <label for="all-receiver">统一转交给</label
            ><select id="all-receiver" v-model="receiver" @change="assignAll">
              <option value="">选择接收者</option>
              <option v-for="u in recipients" :key="u.id" :value="u.id">
                {{ u.username }}
              </option>
            </select>
          </div>
          <div v-for="p in owned" :key="p.id" class="ui-field">
            <label :for="'transfer-' + p.id">{{ p.project_name }}</label
            ><select
              :id="'transfer-' + p.id"
              v-model="transfers[p.id]"
              required
            >
              <option value="">选择接收者</option>
              <option v-for="u in recipients" :key="u.id" :value="u.id">
                {{ u.username }}
              </option>
            </select>
          </div>
        </template>
        <p v-else class="ui-muted">该用户没有需要转交的项目。</p>
      </template>
      <div v-if="error" class="ui-error" role="alert">{{ error }}</div>
    </form>
    <template #footer
      ><button :disabled="loading" @click="requestClose">取消</button
      ><button
        form="user-form"
        type="submit"
        :class="mode === 'delete' ? 'danger' : 'primary'"
        :disabled="loading"
      >
        {{
          loading ? "正在保存…" : mode === "delete" ? "转交并删除账号" : "保存"
        }}
      </button></template
    >
  </Dialog>
  <Dialog
    :open="discardOpen"
    title="放弃未保存的修改？"
    @close="discardOpen = false"
  >
    <p>关闭后将丢弃当前账号表单的输入。</p>
    <template #footer>
      <button @click="discardOpen = false">继续编辑</button>
      <button class="danger" @click="close">放弃修改</button>
    </template>
  </Dialog>
</template>
