# 把一个 Rust CLI 发到 NPM：Trusted Publishing 时代的完整路径

npm 早就不止是 JavaScript 包的家了。esbuild、swc、biome、turbo——这些 Rust/Go 写的工具都走 npm 作为多平台 CLI 的主分发渠道。
原因很现实：用户 `npm install -g` 一句，不需要本地装 cargo 或者 Go 工具链，也不需要识别自己是 arm64 还是 x64。

但发一个 Rust CLI 到 npm 不是 `npm publish` 一句话能搞定的。
要拼起来三块：跨平台预编译矩阵、optionalDependencies 平台分发、以及——从 2025 起官方推荐的——OIDC Trusted Publishing。
我这次给 `claude-quota-bar`（一个 Rust 写的 Claude Code 状态栏）从零跑了一遍完整流程，每块都有几个不显眼的坑。

## Why npm at all?

CLI 工具的多平台分发有几条候选路径：

- **`cargo install`**：用户机器得先装 Rust 工具链，且每次都要本地编译。冷装一个小工具 5 分钟起步。
- **Homebrew**：写 formula、配 SHA256、维护 tap 仓库。macOS / Linuxbrew 各一份，Windows 没了。
- **预编译 tarball + README**：把链接贴出来让用户自己解压加 PATH。摩擦最大。
- **npm**：用户只装 Node（多数前端/全栈机器本来就有），`npm install -g <pkg>` 一句完事。

第四条几乎是唯一一条让安装体验**和工具实现语言无关**的路径。
代价是你要在 release pipeline 里多花一点功夫——但那是一次性的。

读到这你可能会想到 `napi-rs` 和 `cargo-dist`。这两个不在候选里：napi-rs 是给 Node 原生模块用的 N-API binding，场景和 CLI 不重叠；cargo-dist 自带跨平台 release 流程但产物不走 npm，装出来的二进制还是要让用户解压加 PATH——回到 tarball 的摩擦。

## One umbrella, seven platforms

npm 原生支持「按平台选包」。秘诀是 `optionalDependencies` + 子包的 `os` / `cpu` 字段。结构长这样：

```
npm/
├── main/                              # umbrella, name = claude-quota-bar
│   └── package.json
└── platforms/
    ├── darwin-arm64/package.json      # name = claude-quota-bar-darwin-arm64
    ├── darwin-x64/package.json
    ├── linux-x64/package.json
    ├── linux-x64-musl/package.json
    ├── linux-arm64/package.json
    ├── linux-arm64-musl/package.json
    └── win32-x64/package.json
```

umbrella 包的 `package.json` 关键字段：

```json
{
  "name": "claude-quota-bar",
  "version": "0.1.0",
  "bin": { "claude-quota-bar": "./bin.js" },
  "optionalDependencies": {
    "claude-quota-bar-darwin-arm64": "0.1.0",
    "claude-quota-bar-darwin-x64": "0.1.0",
    "claude-quota-bar-linux-x64": "0.1.0",
    "...": "..."
  }
}
```

平台子包的 `package.json` 直接指向预编译二进制：

```json
{
  "name": "claude-quota-bar-darwin-arm64",
  "version": "0.1.0",
  "os": ["darwin"],
  "cpu": ["arm64"],
  "bin": { "claude-quota-bar": "bin/claude-quota-bar" }
}
```

umbrella 的 `bin` 不能直接指向某个平台的二进制——`bin` 字段是静态的，npm 没有「按当前 os/cpu 动态选」的语法。
解决办法是一个 Node shim `bin.js`，做三件事：检测 `process.platform` / `process.arch`，Linux 上额外探测 musl vs glibc，`require.resolve` 到对应平台子包再 `spawn` 它的二进制。完整文件含错误分支大概六七十行，但核心 dispatch 只有这一段：

```js
function detectLibc() {
  if (process.platform !== "linux") return "gnu";
  try { return fs.readdirSync("/lib").some(e => e.startsWith("ld-musl-")) ? "musl" : "gnu"; }
  catch { return "gnu"; }
}
const { platform, arch } = process;
const suffix = platform === "linux" && detectLibc() === "musl" ? "-musl" : "";
const pkg = `claude-quota-bar-${platform}-${arch}${suffix}`;
const binName = platform === "win32" ? "claude-quota-bar.exe" : "claude-quota-bar";
const binPath = require.resolve(`${pkg}/bin/${binName}`);
spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });
```

两个不显眼但抄错就报错的细节：Linux 探测 musl 要扫 `/lib` 找 `ld-musl-*` loader（`/etc/os-release` 在 distroless 容器里没有），Windows 的二进制带 `.exe` 后缀其余平台没有——这两点错一个 launcher 就找不到二进制。

npm 装 umbrella 包时会遍历 `optionalDependencies`，跳过 `os` / `cpu` 不匹配的子包，只装当前平台那一个。
失败的 optional 不会让整次安装失败。

这条路线避开了 `postinstall` 脚本去运行时下载二进制——后者既要打通网络又要处理 checksum，还容易触发企业网络的 SSL 拦截。
代价只是一个不会怎么变的启动器。

## Cross-compile from one runner

7 个 target 不需要 7 台 runner。GitHub Actions 现在的实际情况是：

| Target | Runner | 编译方式 |
|---|---|---|
| `aarch64-apple-darwin` | `macos-14` | 原生 |
| `x86_64-apple-darwin` | `macos-14` | `rustup target add` 后交叉 |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | 原生 |
| `x86_64-unknown-linux-musl` | `ubuntu-22.04` | `cargo-zigbuild` |
| `aarch64-unknown-linux-gnu` | `ubuntu-22.04` | `cargo-zigbuild` |
| `aarch64-unknown-linux-musl` | `ubuntu-22.04` | `cargo-zigbuild` |
| `x86_64-pc-windows-gnu` | `ubuntu-22.04` | MinGW 交叉 |

两个非显然的取舍：

**Intel Mac 不要用 `macos-13`**。
Intel runner 已经基本退役，排队动辄 1 小时以上。
`macos-14` 跑 Apple Silicon，但 Apple LLVM 工具链同时支持两个架构——
`rustup target add x86_64-apple-darwin` 之后 `cargo build --target x86_64-apple-darwin` 就出 Intel 二进制，整次 release（含 7 平台 matrix + GH Release + NPM publish）实测 2 分 2 秒跑完。

**Linux ARM / musl 用 `cargo-zigbuild` 不用 QEMU**。
Zig 作为 linker 在 Ubuntu x64 runner 上能直接交叉编译到 ARM glibc / x64 musl / ARM musl，无 QEMU、无 Docker，速度和原生差不多。
Windows `x86_64-pc-windows-gnu` 其实也能 zigbuild，但 Ubuntu 自带的 MinGW 工具链一行 `apt install mingw-w64` 就有，这条路径在 Rust 社区被打磨了多年，没必要换。

整个矩阵在 `ubuntu-22.04` + `macos-14` 两种 runner 上跑完，没有任何自托管 runner。

## Trusted Publishing isn't a switch

npm 在 2025 年正式推了 Trusted Publishing：GitHub Actions 用 OIDC 短期 token 换 npm 发布权限，仓库里再也不需要存 `NPM_TOKEN`。攻击面消失，provenance attestation 自动生成。

但 TP 不是一个开关。它要求**包必须已经在 npm 注册过**才能在界面里配——一个全新的包名是配不上 TP 的。这是个鸡生蛋。

官方推荐的解法是「先用经典 token 做一次 bootstrap 发布，之后切换到 TP」。我的做法是发占位版本：

```bash
# scripts/bootstrap-npm.sh 核心片段
BOOTSTRAP_VERSION="0.0.0-bootstrap"
for plat in darwin-arm64 darwin-x64 linux-x64 \
            linux-x64-musl linux-arm64 linux-arm64-musl win32-x64; do
  dir="npm/platforms/${plat}"
  mkdir -p "${dir}/bin"
  echo 'bootstrap placeholder' > "${dir}/bin/.placeholder"
  # 把 version 改成 0.0.0-bootstrap，publish 后再 git checkout 还原
  ( cd "${dir}" && npm publish --access=public )
done
```

**顺序很重要**：先把 8 个包（umbrella + 7 个平台）全部在本地 `npm publish` 一份 `0.0.0-bootstrap`——这一步本地登录走一次交互式 2FA 即可。占位版本留在 npm 上不会影响任何人——`0.1.0 > 0.0.0`，正式版本一上来用户 `npm install` 自动拿新版。

8 个占位全部发完后，再到每个包的 `npmjs.com/package/<name>/access` 页面配 Trusted Publisher：填仓库 `org/repo`、workflow filename `release.yml`、environment 留空。8 个包重复 8 次填表——慢，但只做一次。

CI 端 workflow 要的就两块：

```yaml
permissions:
  contents: read
  id-token: write    # OIDC 必需

steps:
  - uses: actions/setup-node@v6
    with:
      node-version: "24"
      registry-url: "https://registry.npmjs.org"   # 必需，否则 npm 不知道往哪发
  - run: node npm/scripts/prepare-packages.js      # 内部循环调 npm publish
```

`id-token: write` 给 workflow 申请 OIDC token 的权限；`registry-url` 让 `setup-node` 写一份 `.npmrc` 指向正确 registry。两个都不能漏。

发完之后 `npm view <pkg> dist.attestations` 应该看到 `provenance: { predicateType: 'https://slsa.dev/provenance/v1' }`——这条 attestation 是攻击者无法伪造的，因为是 GitHub OIDC + workflow 签名。

## Don't upgrade npm in place

OIDC Trusted Publishing 需要 npm ≥ 11.5.1。`actions/setup-node@v6` 在 Node 22 的镜像里带的是 npm 10.x，第一次跑 release 我手贱加了：

```yaml
- run: npm install -g npm@latest
```

CI 报错：

```
Cannot find module 'promise-retry'
```

`npm@latest` 升级自己时把 `node_modules` 改了一半，把 `promise-retry` 给丢了——这是 npm self-upgrade 的著名旧 bug，GitHub issue 一搜一堆，本地原地升级也偶尔会撞上。两个修法：

```yaml
# ❌ 不要原地升级 npm
node-version: "22"
- run: npm install -g npm@latest

# ✅ 换更新的 Node 镜像，npm 一起来
node-version: "24"     # Node 24 自带 npm 11+
```

第二种简单粗暴，但把 npm 当作 Node 镜像的一部分声明出来，比运行时原地改包更可重现。一句话：基底是声明式的，运行时不要去改。

## A cheat sheet

下次新建一个 Rust（或任何编译型语言）CLI 想发到 npm：

1. **包结构**：一个 umbrella + 每平台一个子包；用 `optionalDependencies` + `os` / `cpu` 让 npm 自己挑。不要写 `postinstall` 下载二进制。
2. **矩阵**：`macos-14` 一台跑 Apple Silicon + Intel 交叉；`ubuntu-22.04` + `cargo-zigbuild` 覆盖 Linux ARM/musl；MinGW 交叉编 Windows。Intel Mac runner 已不值得等。
3. **Trusted Publishing 三步走**：本地 `0.0.0-bootstrap` 占位发布 → 网页界面配 8 个包的 Trusted Publisher → CI workflow 加 `permissions: id-token: write` + `setup-node registry-url`。
4. **不要在 CI 里原地升级 npm**。换更新的 Node 镜像（`node-version: "24"` 自带 npm 11+）。
5. **Provenance 是免费的**：OIDC 路径走通后，每个包自动带 SLSA v1 attestation，用户能在 npmjs.com 看到 ✅。
6. **占位包不要删**。npm 不允许重发同一个 version 号，unpublish 之后 24 小时内还能再发，过了 24 小时这个版本号就永久占用了——删完发不回来。`0.0.0-bootstrap` 是 prerelease 标识，semver 上低于 `0.1.0`，用户 `npm install` 自动拿正式版，留着无害。

整个流程的不可替代部分就是 bootstrap 那一次本地发布——一旦走完，仓库里没有任何长期凭据，所有后续 release 都是 `git tag vX.Y.Z && git push --tags` 一句话。

希望对你下次发包有帮助，有问题随时聊。
