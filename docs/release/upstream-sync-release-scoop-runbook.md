# Upstream 同步、代码审计、Release 与 Scoop 发布流程

> 本文是 Codex++ 的可复用发布 runbook。它记录“同步上游 → 审计 → 推送 main → 打 tag → 监控 CI → 更新 Scoop”的顺序、停止条件和经验教训。

## 1. 发布边界

本仓库同时存在两个远程：

- `origin`：本项目发布仓库（当前为 `pedoc/CodexPlusPlus`）
- `upstream`：上游仓库（当前为 `BigPizzaV3/CodexPlusPlus`）

**发布前必须统一发布归属。** 以下内容必须指向同一个发布仓库：

- `crates/codex-plus-core/src/update.rs` 的更新源和可信 Release owner
- `.github/workflows/release-assets.yml` 的 Release 上传目标
- README 的 Release 链接
- Scoop manifest 的下载 URL
- GitHub Actions 使用的 tag

上游代码、主题市场和脚本市场可以继续指向上游项目；它们不等于应用二进制的发布仓库。

## 2. 变量与证据表

每次执行时先记录以下信息，便于审计和回滚：

| 项目 | 值 |
|---|---|
| 发布仓库 | `pedoc/CodexPlusPlus` |
| 上游仓库 | `BigPizzaV3/CodexPlusPlus` |
| 同步前 `main` SHA | `<record>` |
| `upstream/main` SHA | `<record>` |
| 合并提交 SHA | `<record>` |
| 发布版本 | `v<major>.<minor>.<patch>` |
| tag 目标 SHA | `<record>` |
| Release workflow run | `<URL / run id>` |
| Windows ZIP SHA-256 | `<record>` |
| Scoop manifest commit | `<record>` |

## 3. Preflight：先保护状态

在仓库根目录执行：

```bash
git status --short --branch
git remote -v
git branch -vv
git log -8 --oneline --decorate
git tag --sort=-version:refname | head -20
```

停止条件：

- 工作树存在无法解释的修改或未跟踪文件；
- 当前已有 merge/rebase/cherry-pick 在进行；
- `origin/main` 在审计期间意外前进；
- 目标版本 tag 已存在；
- Scoop bucket 工作树不干净。

建议在开始前创建本地恢复分支：

```bash
git branch backup/release-<date>
git switch -c integrate/upstream-<date>
```

恢复分支不要推送到远程，也不要使用 `v*` 命名，避免触发发布 workflow。

## 4. Fetch 与合并上游

不要用会自动改写本地分支的 `pull` 替代审计流程：

```bash
git fetch upstream

git rev-list --left-right --count main...upstream/main
git log --oneline main..upstream/main
git diff --stat main...upstream/main
```

在 integration 分支合并：

```bash
git merge --no-ff upstream/main
```

冲突处理原则：

1. 先理解本地提交为什么存在；
2. 不要无条件选择 `ours` 或 `theirs`；
3. 广告、更新器、脚本、认证、文件删除、发布 workflow 属于高风险冲突；
4. 解决后检查：

```bash
git diff --check
git diff --name-only --diff-filter=U
git grep -n -E '^(<<<<<<<|=======|>>>>>>>)' -- ':!node_modules'
```

合并提交必须保留清晰的 merge parent。不要在 `main` 上 force-push。

## 5. 代码与安全审计

### 5.1 一般质量检查

```bash
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings

cd apps/codex-plus-manager
npm ci
npm test
npm run check
npm run vite:build
npm audit --audit-level=high
cd ../..
```

上游可能携带既有格式或 warning。不能把既有问题误判为本次改动无关紧要；要记录并决定是否修复或阻止发布。

### 5.2 必审安全面

重点查看：

- 更新器：URL host/path、tag、平台资产、大小、SHA-256、安装包签名、是否会启动；
- GitHub Actions：`contents: write`、不可信输入、shell/PowerShell 插值、checkout ref、凭据持久化；
- 文件系统：路径遍历、symlink、数据库路径、rollout 路径、备份恢复；
- 脚本市场：是否自动下载/安装/执行、来源 allowlist、hash、运行时完整性；
- Renderer 注入：远程内容、`innerHTML`、外部 URL、脚本权限；
- 微信/远程控制：空白白名单、`*`、sandbox、approval policy；
- 诊断和日志：API key、token、auth/config 内容、本地路径；
- 外部服务：分享站、远程市场、供应商 API 的数据流和限流。

建议使用独立的 general code review 和 security review。任何 Critical/High finding 都必须在 push/tag 前解决或明确停止发布。

## 6. 版本与 tag

tag 必须与所有内嵌版本一致：

- 根 `Cargo.toml` / `Cargo.lock`
- `apps/codex-plus-manager/package.json`
- `apps/codex-plus-manager/package-lock.json`
- `apps/codex-plus-manager/src-tauri/tauri.conf.json`

检查：

```bash
cargo metadata --no-deps --format-version 1
npm --prefix apps/codex-plus-manager pkg get version
```

版本选择规则：

- bug/security 修复：patch，例如 `1.3.1`；
- 向后兼容功能：minor；
- 破坏性变更：major。

先查询远程 tag 和 Release，确认新 tag 未使用：

```bash
git fetch origin --tags
git ls-remote --tags origin
 gh release list --repo pedoc/CodexPlusPlus
```

不要移动已发布 tag。若版本 tag 已存在，选择新的 patch tag，不要 `git tag -f` 或 force-push。

## 7. 推送顺序

### 7.1 发布 main

在最终审计前再次确认 remote 未前进：

```bash
git fetch origin
git status --short --branch
git rev-parse HEAD
git rev-parse origin/main
```

推送时不使用 force：

```bash
git push origin main
```

随后等待 main 分支 CI 全部成功，再创建 release tag。

### 7.2 创建并推送 tag

tag 必须指向已经审计且已存在于远程 `main` 的 commit：

```bash
git rev-parse origin/main
git tag -a v1.3.1 origin/main -m "Codex++ v1.3.1"
git push origin refs/tags/v1.3.1
```

若没有签名环境，至少使用 annotated tag。tag 推送后不要移动它。

## 8. 监控 Release workflow

查找 run：

```bash
gh run list --repo pedoc/CodexPlusPlus \
  --workflow release-assets.yml --limit 5 \
  --json databaseId,status,conclusion,headBranch,url
```

持续监控：

```bash
gh run watch <run-id> --repo pedoc/CodexPlusPlus --exit-status
```

必须成功的 job：

- `windows-installer`
- `macOS DMG (x64)`
- `macOS DMG (arm64)`
- `Upload static latest.json`

若失败：

- 先读失败步骤和日志；
- 若代码正确、只是基础设施失败，可对同一个 immutable tag 重跑；
- 不要修改 tag 指向另一个 commit；
- 若代码有问题，修复后发布新的 patch tag。

## 9. 验证 Release 和 latest.json

列出 assets：

```bash
gh release view v1.3.1 --repo pedoc/CodexPlusPlus \
  --json tagName,isDraft,isPrerelease,url,assets
```

预期资产：

- `CodexPlusPlus-1.3.1-windows-x64-setup.exe`
- `CodexPlusPlus-1.3.1-windows-x64.zip`
- `CodexPlusPlus-1.3.1-macos-x64.dmg`
- `CodexPlusPlus-1.3.1-macos-x64.zip`
- `CodexPlusPlus-1.3.1-macos-arm64.dmg`
- `CodexPlusPlus-1.3.1-macos-arm64.zip`
- `latest.json`

检查：

1. 所有资产 state 为 `uploaded` 且 size 大于 0；
2. URL 使用 HTTPS 且指向 `pedoc/CodexPlusPlus/releases/download/<tag>/...`；
3. GitHub asset digest、`latest.json` 的 `sha256`、Scoop hash 三者一致；
4. `latest.json` 版本与 tag 完全一致；
5. updater 的 `DEFAULT_LATEST_JSON_URL` 和可信 owner 与发布仓库一致。

示例：

```bash
tmp=$(mktemp -d)
gh release download v1.3.1 --repo pedoc/CodexPlusPlus \
  --pattern latest.json --dir "$tmp"
python - "$tmp/latest.json" <<'PY'
import json, sys
payload = json.load(open(sys.argv[1], encoding="utf-8"))
assert payload["version"] == "v1.3.1"
for asset in payload["assets"]:
    assert len(asset["sha256"]) == 64
print("latest.json verified")
PY
```

## 10. 更新 Scoop bucket

外部 bucket：

```text
C:\Users\pedoc\scoop\buckets\pedoc
```

先保护 bucket 状态：

```bash
cd /c/Users/pedoc/scoop/buckets/pedoc
git status --short --branch
git fetch origin
```

如果工作树不干净，停止，不要 stash、reset 或覆盖未知修改。

编辑：

```text
bucket/CodexPlusPlus.json
```

更新：

- `version` 为新版本，不带 `v`；
- 64-bit URL 指向已验证的 Windows ZIP；
- `hash` 使用该 ZIP 的独立 SHA-256；
- 保留 autoupdate URL 模板和 shortcuts；
- homepage/release owner 与实际发布仓库一致。

验证：

```bash
python -m json.tool bucket/CodexPlusPlus.json >/dev/null
scoop checkver CodexPlusPlus
scoop update CodexPlusPlus
```

确认安装结果同时包含：

- `codex-plus-plus.exe`
- `codex-plus-plus-manager.exe`

提交并推送 bucket：

```bash
git add bucket/CodexPlusPlus.json
git commit -m "chore(bucket): update CodexPlusPlus to 1.3.1"
git push
```

## 11. 停止条件与回滚

发布前必须停止：

- 工作树或 Scoop bucket 有未知修改；
- merge 有未解决冲突；
- 版本、tag、远程仓库不一致；
- 安全审计出现 Critical/High；
- main CI 或 Release workflow 失败；
- 资产缺失、hash 不一致或 latest.json 错误；
- Scoop 安装/更新失败。

回滚原则：

- 未推送前：使用本地 backup 分支恢复；
- main 已推送：追加修复或 revert，不 force-push；
- tag 已推送：不移动 tag，修复后发布新 patch tag；
- Release 资产错误：先停止 Scoop 更新，不要静默替换另一个 commit 下的同名 tag。

## 12. 本次经验教训

1. **远程 tag 可能和本地 tag 冲突。** Fetch tags 失败时先区分“分支更新”和“tag clobber”，不要强制覆盖已有 tag。
2. **不要把上游仓库当作发布仓库。** 本项目的 upstream 是 `BigPizzaV3/CodexPlusPlus`，origin/Scoop 发布仓库是 `pedoc/CodexPlusPlus`；更新器、workflow、README 和 Scoop 必须明确归属。
3. **发布前必须把版本号和 tag 同步。** 不能沿用 `v1.3.0` tag 发布 `1.3.1` 代码，也不能只改 tag 不改内嵌版本。
4. **Release workflow 必须从 immutable tag 构建。** 手动输入的 tag 要在 checkout 前验证，并使用 `refs/tags/<tag>`；checkout 不应持久化写权限凭据。
5. **`latest.json` 必须包含可独立验证的 SHA-256。** 不能只验证 URL 或文件名；updater 和 Scoop 都要使用同一已验证摘要。
6. **用户明确安装的脚本和宿主自动行为要区分。** 宿主不应自动下载/执行远程市场脚本；用户主动安装后仍需保留完整性校验和运行时校验。
7. **安全修复要单独提交。** 先做可审计的 upstream merge，再用独立提交修复安全发现，便于回溯、审查和回滚。
8. **CI 不能只看 job 名称。** 必须验证 Windows、两个 macOS 架构、latest.json 和最终 Release asset digest。

## 13. 完成记录模板

```text
Baseline main:       <sha>
Fetched upstream:    <sha>
Merge commit:        <sha>
Audited main:        <sha>
Pushed origin/main:  <sha>
Release tag:         v<version>
Tag target:          <sha>
Workflow run:        <url>
Release URL:         <url>
Windows ZIP SHA256:  <sha256>
Scoop commit:        <sha>

General tests:       pass/fail
Security review:     pass/fail
Dependency audit:    pass/fail
CI Release assets:   pass/fail
Scoop validation:    pass/fail
```
