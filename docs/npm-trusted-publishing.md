# NPM Trusted Publishing 一次性配置

Trusted Publishing 用 OIDC 把 GitHub Actions 与 npm 建立信任，**无需任何长期 token**。
配置一次后，所有 release 都用 workflow attestation 发布。

## 前置

- 你已登录 npm 账号（[npmjs.com](https://www.npmjs.com)）
- 此账号准备承担这 8 个包的 owner：
  - `claude-quota-bar` (umbrella)
  - `claude-quota-bar-darwin-arm64`
  - `claude-quota-bar-darwin-x64`
  - `claude-quota-bar-linux-x64`
  - `claude-quota-bar-linux-x64-musl`
  - `claude-quota-bar-linux-arm64`
  - `claude-quota-bar-linux-arm64-musl`
  - `claude-quota-bar-win32-x64`

## 步骤

### 1. Bootstrap 占位发布（本地一次）

Trusted Publishing 需要包先在 npm 注册过。用本地 `npm login` 发一份 `0.0.0-bootstrap` 占位版本。

```sh
# 本地登录 npm（普通流程，2FA 交互式输入即可，不需 bypass）
npm login

# 一次发完所有 8 个占位包
cd ~/developer/claude-quota-bar
./scripts/bootstrap-npm.sh

# 收尾——删除本地长期凭据
npm logout
```

### 2. 给每个包加 Trusted Publisher

8 个包都要做：

1. 打开 `https://www.npmjs.com/package/<包名>/access`
2. 找到 **Trusted publishers** 区块 → **Add new publisher**
3. 填写：
   - **Provider**: GitHub Actions
   - **Organization or user**: `xrf9268-hue`
   - **Repository**: `claude-quota-bar`
   - **Workflow filename**: `release.yml`
   - **Environment name**: 留空（除非你配了 GH Environment）
4. **Save**

8 个包重复 4 次填表。建议用浏览器记录密码后批量复制粘贴。

### 3. 触发发布

发布不再手动打 tag。`release-plz` 在每次 push 到 `main` 时维护一个 **Release PR**（bump 版本 + 更新 `CHANGELOG.md`）；**合并那个 PR** 就会打 tag、建 GitHub Release，并在同一 workflow run 里跑 `publish-npm`（OIDC，无 `NPM_TOKEN`）。

## 验证

- 任意机器 `npm install -g claude-quota-bar` 应能拉到 0.1.0
- npm 包页面应有 ✅ "Provenance" 标记（攻击者无法伪造，因为是 OIDC + workflow attestation）

## 出问题排查

- **CI 报 `403 npm-trusted-publisher-not-configured`**: 那个包的 Trusted Publisher 没配好，回到 step 2 检查 GH repo / workflow filename 完全一致
- **CI 报 `OIDC token not found`**: workflow 漏了 `permissions: id-token: write`（已配，理论上不会触发）
- **占位 0.0.0 一直留在 npm**: 不影响——`0.1.0 > 0.0.0`，用户 `npm install` 自动拿新版

## crates.io Trusted Publishing（一次性）

`release-plz` 也会把 crate 发到 crates.io。crates.io 同样支持 OIDC Trusted Publishing，且 release-plz 自己完成 token 交换——**不需要 `CARGO_REGISTRY_TOKEN`**，只需 release-plz job 有 `id-token: write`（已配）。

一次性步骤：

1. **Bootstrap 当前版本**：让 crate 先在 crates.io 存在于当前版本（`0.3.0`），这样 release-plz 的基线与现状一致，首次自动运行不会误判「未发布」而尝试重发 `0.3.0`（会和已存在的 `v0.3.0` tag 冲突）。本地一次：
   ```sh
   cargo login        # 临时 classic token，发完即可撤销
   cargo publish      # 发布当前 Cargo.toml 版本到 crates.io
   ```
2. **配 Trusted Publisher**：crates.io 上 `claude-quota-bar` 的 **Settings → Trusted Publishing → Add**：
   - **Repository owner**: `xrf9268-hue`
   - **Repository name**: `claude-quota-bar`
   - **Workflow filename**: `release.yml`
   - **Environment**: 留空
3. 之后所有 release 由 release-plz 经 OIDC 自动发布，无长期 token。

## 回退到经典 Token

如果 Trusted Publishing 配不通：
1. 创建经典 token (Settings → Access Tokens → Granular Access Token)，**勾选 Bypass 2FA**，30 天过期
2. GH repo → Settings → Secrets → New `NPM_TOKEN`
3. 把 release.yml 改回带 `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` 的版本（git revert 这个 commit 即可）
