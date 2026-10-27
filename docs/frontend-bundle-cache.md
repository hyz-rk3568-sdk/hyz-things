# 前端产物复用与 Portal CI

## 本地产物

`apps/rust/things/tools/build-frontend-bundle.sh` 管理两个配套文件：

- `target/frontend-bundle/hyz-things-frontend.tar`：确定性前端归档。
- `target/frontend-bundle/hyz-things-frontend.json`：版本化的输入、构建工具链和归档 SHA-256 指纹。

运行 `make things-frontend` 时，脚本先校验指纹。输入、工具链和归档校验值全部匹配时直接复用，跳过 CSS、Trunk 和 WASM 构建。没有元数据、输入变化、归档变化或元数据损坏时，进入原有构建流程。已有但未经记录的 tar 不会被直接认作有效缓存。

输入指纹按工作区相对路径和文件内容计算，覆盖 things 包源码、前端资源、构建脚本、测试源码、依赖清单和锁文件，以及 contract 源码和清单。测试源码也参与计算，因为 Tailwind 会自动扫描包内源文件。祖先目录的 Cargo 配置、Rust 工具链文件和 `.gitignore` 同样参与校验。新增、删除和改名都能使缓存失效，单纯修改文件时间不会失效。

遍历时剪枝 `target`、`node_modules`、Playwright 报告、`__pycache__` 和 `.git`，并排除生成的 `frontend/router.css`。不支持的源码目录符号链接和不可读取输入会使校验失败。

本机构建工具链指纹包含 Rust、Node、npm、Trunk 版本、Cargo 用户配置及 Rust/Trunk 的相关构建参数。只记录摘要，不将配置或环境变量原文写入元数据。

构建期间以 `.build.lock` 串行化归档和元数据发布。构建结束后再次确认输入和工具链未变化。生成 tar 与旧 tar 字节相同时保留旧文件及其 mtime，更新配套元数据；只有归档内容变化时才原子替换 tar。这样不会因为相同归档再次打包而触发 `build.rs` 的输入文件时间变化。

## E2E 启动

默认 `npm run test:e2e` 仍通过构建脚本准备前端，现在会先尝试上述校验复用。本地默认路径仍需要现有构建工具和前端依赖通过检查。

明确使用预构建产物时：

```sh
cd apps/rust/things
HYZ_THINGS_E2E_REUSE_FRONTEND_BUNDLE=1 npm run test:e2e
```

该模式只验证当前源文件指纹和归档 SHA-256，然后启动 Rust E2E 服务。它不安装或调用前端构建工具；构建工具链摘要作为产物来源信息保存在元数据中。缺少配套元数据或校验失败时，在启动 Cargo 之前退出，不会偷偷使用旧页面或回退到重建。

首次启用该机制需要重新构建一次以产生可信元数据。不要手工给旧 tar 补写元数据。

## Portal CI 分工

工作流保留三个 job：

1. `frontend`：一次构建/验证前端，上传 tar 和 JSON 元数据。归档缓存与大型 Cargo 缓存分开，归档缓存 key 使用实际输入和工具链指纹；校验命中后跳过 npm 依赖安装、WASM target 安装及前端构建。
2. `checks`：集中执行 diff、静态、Rust 格式、contract/Web/native 单元与 HTTP 测试和 Clippy。它等待前端任务完成，以便 native HTTP 测试能够使用真实资源；前端失败时仍尝试收集独立检查结果。
3. `e2e`：只依赖成功的前端产物，独立运行 Playwright，不等待 `checks`。

检查步骤使用带 `!cancelled()` 的条件，格式或静态检查失败后，具备必要工具的后续测试仍执行。失败步骤保留失败状态，整个 job 仍然失败。取消运行会停止后续检查。没有用 `continue-on-error` 把质量检查变成成功。

## Host 验证

```sh
python3 apps/rust/things/tools/test-frontend-bundle.py
python3 apps/rust/things/tools/test-portal-ci.py
make check-static
```

这些测试使用 fake Node/npm/Trunk/Rust/Cargo 命令及临时工作区，验证复用、失效、归档完整性、mtime、构建失败、输入中途变化和互斥发布。CI 回归还会模拟格式及静态检查失败，确认所有单元测试命令继续执行并保留失败结果。这些测试不启动真实构建、浏览器或设备服务；真实构建耗时和 GitHub runner 行为需要另行运行 CI 验证。
