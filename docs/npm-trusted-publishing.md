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

### 3. 测试发布

```sh
git tag v0.1.0
git push --tags
```

进 GitHub Actions → Release workflow → publish-npm 这一 job 应该用 OIDC 发布成功，不需要任何 `NPM_TOKEN` secret。

## 验证

- 任意机器 `npm install -g claude-quota-bar` 应能拉到 0.1.0
- npm 包页面应有 ✅ "Provenance" 标记（攻击者无法伪造，因为是 OIDC + workflow attestation）

## 出问题排查

- **CI 报 `403 npm-trusted-publisher-not-configured`**: 那个包的 Trusted Publisher 没配好，回到 step 2 检查 GH repo / workflow filename 完全一致
- **CI 报 `OIDC token not found`**: workflow 漏了 `permissions: id-token: write`（已配，理论上不会触发）
- **占位 0.0.0 一直留在 npm**: 不影响——`0.1.0 > 0.0.0`，用户 `npm install` 自动拿新版

## 回退到经典 Token

如果 Trusted Publishing 配不通：
1. 创建经典 token (Settings → Access Tokens → Granular Access Token)，**勾选 Bypass 2FA**，30 天过期
2. GH repo → Settings → Secrets → New `NPM_TOKEN`
3. 把 release.yml 改回带 `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}` 的版本（git revert 这个 commit 即可）
