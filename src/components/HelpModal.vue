<script setup lang="ts">
import { ref } from "vue";

defineProps<{ show: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const tab = ref<"intro" | "local" | "remote">("intro");
</script>

<template>
  <Teleport to="body">
    <div v-if="show" class="help-overlay" @click.self="emit('close')">
      <div class="help-modal">
        <div class="help-header">
          <h2>使用指南</h2>
          <button class="help-close" @click="emit('close')">&times;</button>
        </div>

        <div class="help-tabs">
          <button :class="{ active: tab === 'intro' }" @click="tab = 'intro'">项目简介</button>
          <button :class="{ active: tab === 'local' }" @click="tab = 'local'">本地模式</button>
          <button :class="{ active: tab === 'remote' }" @click="tab = 'remote'">远程模式</button>
        </div>

        <div class="help-body">
          <!-- 简介 -->
          <section v-if="tab === 'intro'">
            <p class="lead">
              这个工具用来给 Unity YooAsset 热更资源做<strong>测试服务</strong>和<strong>分发服务</strong>。
              它有两种模式，按你的需求选一种用。
            </p>

            <div class="cards">
              <div class="card">
                <div class="card-title">🏠 本地模式</div>
                <div class="card-desc">
                  在自己电脑上启动一个 HTTP 服务器，把本机的 Bundles 目录暴露出来。
                  Unity 编辑器或同局域网的手机直接连本机就能下到资源。
                  <strong class="card-tag">适合：</strong>个人开发、本机调试、小范围测试
                </div>
              </div>
              <div class="card">
                <div class="card-title">☁️ 远程模式</div>
                <div class="card-desc">
                  连接到部署在云服务器或 NAS（如群晖）的实例，把本地资源上传到远端。
                  所有玩家都能从远端下载，配合 Docker 一键部署。
                  <strong class="card-tag">适合：</strong>团队协作、外网测试、正式分发
                </div>
              </div>
            </div>

            <h3>几个关键概念</h3>
            <ul class="concepts">
              <li><strong>项目</strong>：一组配置（项目名 + 包名 + 平台），对应 Unity 里一个 YooAsset Package</li>
              <li><strong>Bundles 目录</strong>：Unity Build 输出的资源根目录，里面按平台/包名/版本分文件夹</li>
              <li><strong>版本</strong>：Unity 每次 Build 产生一个版本快照，工具会保留多个版本方便切换</li>
              <li><strong>激活版本</strong>：当前正在对外提供下载的版本（Unity 客户端实际拿到的就是这个）</li>
            </ul>

            <p class="hint">
              💡 不知道选哪个模式？先用<strong>本地模式</strong>跑通整个流程，等需要给别人测试时再切到远程模式。
            </p>
          </section>

          <!-- 本地模式 -->
          <section v-if="tab === 'local'">
            <p class="lead">
              本地模式 = 在你自己电脑上跑一个静态文件服务器。资源放本地，URL 给 Unity 用。
            </p>

            <div class="step-simple">
              <div class="step-num">1</div>
              <div class="step-text">
                <strong>Unity 中构建资源</strong>
                <p>用 YooAsset 的 AssetBundle Builder 构建一次。记住输出的 Bundles 目录位置。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">2</div>
              <div class="step-text">
                <strong>软件中配置项目</strong>
                <p>填写项目名、包名（默认 DefaultPackage）、勾选目标平台，端口默认 8081。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">3</div>
              <div class="step-text">
                <strong>选择 Bundles 目录</strong>
                <p>点「浏览」按钮，选第 1 步 Unity 输出的 Bundles 目录。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">4</div>
              <div class="step-text">
                <strong>同步资源</strong>
                <p>点「↻ 同步资源」按钮，选要测试的版本（默认选最新），软件会把该版本文件链接到服务目录。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">5</div>
              <div class="step-text">
                <strong>启动服务</strong>
                <p>点「▶ 启动服务」按钮，绿灯亮起表示运行中。复制显示的 URL，例如 <code>http://127.0.0.1:8081/项目名/Android/</code>。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">6</div>
              <div class="step-text">
                <strong>Unity 中使用</strong>
                <p>把这个 URL 配置到 YooAsset 的 HostPlayMode（IRemoteServices 实现）。Play 运行游戏，资源会从本机服务器下载。</p>
              </div>
            </div>

            <div class="tips-box">
              <h4>💡 常用技巧</h4>
              <ul>
                <li><strong>资源更新</strong>：Unity 重新 Build 后，回到软件点一下「同步资源」就行，不用重启服务</li>
                <li><strong>手机测试</strong>：手机和电脑同 WiFi，URL 里的 127.0.0.1 换成电脑的内网 IP</li>
                <li><strong>多项目</strong>：标签栏 + 号添加多个项目，每个项目用不同端口（8081、8082...）</li>
                <li><strong>版本回滚</strong>：「同步资源」时可以选历史版本，方便对比测试</li>
              </ul>
            </div>
          </section>

          <!-- 远程模式 -->
          <section v-if="tab === 'remote'">
            <p class="lead">
              远程模式 = 把资源上传到一台已部署的服务器（云服务器/群晖 NAS），所有人都能下载。
            </p>

            <div class="prereq">
              <strong>前置条件：</strong>服务器端要先部署好 Docker 镜像。简单的话，群晖 Container Manager 导入镜像就行。具体看部署文档。
            </div>

            <div class="step-simple">
              <div class="step-num">1</div>
              <div class="step-text">
                <strong>Unity 中构建资源</strong>
                <p>跟本地模式一样，用 YooAsset Builder 构建到 Bundles 目录。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">2</div>
              <div class="step-text">
                <strong>切到远程模式</strong>
                <p>软件顶部点「远程模式」按钮。第一次进入会显示连接表单。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">3</div>
              <div class="step-text">
                <strong>登录远程服务器</strong>
                <p>填写连接名称（自己起名，方便区分多个服务器）、服务器地址（如 <code>http://你的IP:端口</code>）、管理密码。可勾选「记住密码」。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">4</div>
              <div class="step-text">
                <strong>新建或编辑项目</strong>
                <p>登录后会看到项目列表。+ 号添加新项目，填写项目名、包名、平台。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">5</div>
              <div class="step-text">
                <strong>选择本地 Bundles 目录</strong>
                <p>点「浏览」选 Unity 输出的 Bundles 目录（**和本地模式一样**，但这次是为了上传，不是本地服务）。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">6</div>
              <div class="step-text">
                <strong>同步资源到远端</strong>
                <p>点「▶ 同步资源」，选要上传的版本。软件会把该版本所有文件 multipart 上传到服务器，**完成后自动激活**。</p>
              </div>
            </div>

            <div class="step-simple">
              <div class="step-num">7</div>
              <div class="step-text">
                <strong>Unity 中使用</strong>
                <p>把远程 URL 配置到 YooAsset，路径格式：<code>http://服务器:端口/res/项目名/平台</code>。任何能访问该地址的设备都能下到资源。</p>
              </div>
            </div>

            <div class="tips-box">
              <h4>💡 常用技巧</h4>
              <ul>
                <li><strong>多服务器切换</strong>：登录页左侧栏可保存多个服务器（测试 / 正式 / 同事的等等），一键切换</li>
                <li><strong>查看资源</strong>：每个版本旁边「浏览文件」可以看具体文件列表，激活版本还能复制单个文件 URL</li>
                <li><strong>版本管理</strong>：所有上传过的版本都保留在服务端，可随时切换激活版本（一键回滚）</li>
                <li><strong>实时日志</strong>：底部日志面板通过 WebSocket 推送服务器实时请求，谁在下载一目了然</li>
                <li><strong>清缓存</strong>：右上角设置 → 清理所有缓存，会清空已保存的服务器和路径记录</li>
              </ul>
            </div>
          </section>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.help-tabs {
  display: flex;
  border-bottom: 1px solid var(--border);
  padding: 0 24px;
}
.help-tabs button {
  padding: 10px 18px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  cursor: pointer;
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
  transition: color 0.15s, border-color 0.15s;
}
.help-tabs button:hover {
  color: var(--text-primary);
}
.help-tabs button.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  font-weight: 600;
}

.help-body section {
  padding: 4px 0;
}

.lead {
  font-size: 14px;
  color: var(--text-primary);
  background: rgba(34, 211, 238, 0.06);
  border-left: 3px solid var(--accent);
  padding: 12px 16px;
  border-radius: 0 8px 8px 0;
  margin: 0 0 20px;
}

.cards {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  margin-bottom: 24px;
}
.card {
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 16px;
}
.card-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--accent);
  margin-bottom: 8px;
}
.card-desc {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.7;
}
.card-tag {
  display: inline-block;
  margin-top: 6px;
  color: var(--text-primary);
}

.concepts {
  list-style: none;
  padding: 0;
  margin: 0 0 16px;
}
.concepts li {
  padding: 8px 12px;
  border-left: 2px solid var(--border);
  margin-bottom: 6px;
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.6;
}
.concepts li strong {
  color: var(--accent);
  margin-right: 4px;
}

h3 {
  font-size: 14px;
  color: var(--text-primary);
  margin: 20px 0 10px;
}

.hint {
  font-size: 12px;
  color: var(--text-muted);
  background: var(--bg-tertiary);
  padding: 10px 14px;
  border-radius: 8px;
  margin-top: 16px;
}

.prereq {
  background: rgba(251, 191, 36, 0.08);
  border: 1px solid rgba(251, 191, 36, 0.25);
  color: #fbbf24;
  padding: 10px 14px;
  border-radius: 8px;
  font-size: 12px;
  margin-bottom: 16px;
}
.prereq strong {
  color: #fcd34d;
  margin-right: 4px;
}

.step-simple {
  display: flex;
  gap: 14px;
  margin-bottom: 14px;
  align-items: flex-start;
}
.step-num {
  flex-shrink: 0;
  width: 26px;
  height: 26px;
  border-radius: 50%;
  background: var(--accent);
  color: var(--bg-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 700;
  margin-top: 1px;
}
.step-text {
  flex: 1;
  min-width: 0;
}
.step-text strong {
  display: block;
  font-size: 13px;
  color: var(--text-primary);
  margin-bottom: 4px;
}
.step-text p {
  font-size: 12.5px;
  color: var(--text-secondary);
  margin: 0;
  line-height: 1.7;
}
.step-text code {
  background: var(--bg-tertiary);
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 11.5px;
  color: var(--accent);
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
}

.tips-box {
  background: var(--bg-tertiary);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 18px;
  margin-top: 20px;
}
.tips-box h4 {
  margin: 0 0 8px;
  font-size: 13px;
  color: var(--accent);
  font-weight: 600;
}
.tips-box ul {
  margin: 0;
  padding-left: 18px;
}
.tips-box li {
  font-size: 12.5px;
  color: var(--text-secondary);
  line-height: 1.8;
  margin-bottom: 4px;
}
.tips-box li strong {
  color: var(--text-primary);
}
</style>
