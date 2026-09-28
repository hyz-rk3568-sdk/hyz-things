# hyz-things 统一错误模型与结构化日志改造计划

## 状态

**计划中，尚未实施。**

创建日期：2026-09-28。

本文用于统一 hyz-things 管理门户后端的错误语义、跨进程错误传播、HTTP 错误 contract 与服务日志。当前代码已经存在若干类型化错误，但错误在跨层传播时会被压缩为通用字符串或通用错误码，导致 Web、日志和自动化无法稳定区分真实失败原因。

本计划优先解决“错误语义在 application / Unix control / HTTP / 日志之间丢失”的问题；结构化日志是统一错误模型的输出层，而不是独立目标。实施时必须保持现有 process boundary、安全边界、router/camera ownership/readiness 语义与 Web 管理权限模型。

实现阶段属于跨模块重构，必须在独立分支完成并通过 Pull Request 合入 main；本文作为纯文档计划可独立提交。

## 1. 当前问题

### 1.1 hyz-things 内部错误已经有一定分类，但没有稳定贯穿到 HTTP

apps/rust/things/src/application/ports.rs 的 PlatformError 已经区分：

- Busy
- Conflict
- ProbeFailed
- CommandFailed
- InvalidState
- NotImplemented
- UnsafeToCutOver
- Io

这类类型化错误适合作为 application / adapter 边界的内部语义，但当前 HTTP 层大量失败最终只映射成少数通用 code，例如：

- control_failed
- service_unavailable
- invalid_request
- forbidden
- not_found

结果是 message 中可能仍包含具体失败文本，但 error.code 无法稳定区分 probe、command、conflict、invalid state 等类别。

### 1.2 router control 错误目前连续丢失语义

当前跨进程链路为：

~~~
hyz-router application / ControlHandler
        │
        │ Result<ControlResult, String>
        ▼
router inbound control
        │
        │ 大多数 handler 错误统一为
        │ code = operation_failed
        ▼
hyz-contract client
        │
        │ ControlError 被格式化进 std::io::Error 字符串
        ▼
hyz-things RouterControlClient
        │
        │ 再次转换为 String
        ▼
hyz-things HTTP
        │
        │ 多数控制失败统一为
        │ code = control_failed
        ▼
Browser
~~~

因此一个原本可以明确判断为网络 probe failure、proxy command failure、generation conflict 或 Tailscale state conflict 的错误，最终只能依赖自然语言 message 判断。

这是本计划最优先修复的错误传播链。

### 1.3 authentication HTTP code 过度合并

AdminError 已经区分：

- InvalidCredentials
- InvalidSession
- RateLimited
- InvalidNewPassword
- PasswordChangeRequired
- Unavailable

HTTP status 已经能够部分区分 400 / 401 / 403 / 429 / 503，但 JSON error.code 当前仍统一使用 authentication_failed。

客户端因此必须结合 endpoint、status 和 message 才能判断错误语义。

### 1.4 Camera HTTP 错误是当前较好的参考基线

Camera 当前已经使用较稳定的业务错误码：

- camera_invalid_request
- camera_not_ready
- camera_busy
- camera_offer_unsupported
- camera_resource_exhausted
- camera_session_not_found
- camera_session_forbidden
- camera_unavailable

本计划应保留这些既有 code，并把其他管理能力逐步收敛到相同原则，而不是重新发明 Camera 错误 contract。

### 1.5 服务日志仍主要依赖自然语言

hyz-things composition root 当前仍直接使用 eprintln! 输出运行事件，例如：

~~~
hyz-things: Tailscale management listener reconcile failed: ...
hyz-things: Tailscale status unavailable: ...
hyz-things: management portal ready at ...
~~~

这些日志适合人工阅读，但缺少稳定字段，无法可靠按 component、operation、error_code 或 request 关联查询，也容易因为 message 文案变化破坏自动诊断。

## 2. 目标

本计划完成以下目标：

1. 建立稳定、可测试的错误分类规则，使错误从 application 到 HTTP / 日志时不再依赖 message 文本识别。
2. 保留 domain/application 自己的类型化错误，不建立一个吞并所有业务语义的全局 mega enum。
3. 让 hyz-router -> hyz-things 的 control 错误保留稳定 remote code，而不是在 client transport 层转成普通 io::Error 字符串。
4. 为 hyz-things HTTP API 建立统一错误 envelope、稳定 error.code 和明确的 HTTP status 映射。
5. 保留 Camera 现有稳定 code，逐步消除不必要的 control_failed / authentication_failed 等过宽 code。
6. 引入结构化日志基础设施，使关键日志至少包含 component、operation、error_code 和必要的关联字段。
7. 为每个 HTTP request 建立服务端生成的关联 ID，使 API 失败可以与服务日志对应。
8. 对 secret、credential、session、CSRF、Tailscale login URL、subscription URL 等敏感字段建立明确的日志脱敏与禁止记录规则。
9. 让 Web 前端根据稳定 error.code 处理已知错误，message 只用于展示或 fallback，不作为程序分支条件。
10. 用 contract/unit/HTTP/E2E 测试固定错误映射和日志脱敏行为。

## 3. 非目标

本计划不实现：

- 修改 router 的网络 reconcile、Mihomo、DHCP、Tailscale、OTA 或 shutdown 业务规则；
- 修改 camera media pipeline、WebRTC 生命周期或端口分配策略；
- 把所有 daemon 的所有历史日志一次性重写；
- 引入远程日志平台、Telemetry backend、OpenTelemetry collector、Sentry 或其他外部服务；
- 在日志中记录完整 HTTP body、完整 control frame 或任意 secret；
- 为每个 Linux errno、shell exit code 或具体命令创建公开 HTTP error code；
- 让前端根据自然语言 message 做稳定逻辑判断；
- 为追求“更详细错误”而向未认证调用方暴露内部路径、命令、credential 状态或其他敏感实现细节；
- 改变现有 API 的权限、CSRF、same-origin、body limit、typed JSON 或 Camera viewer/admin 安全边界。

如果实施过程中发现某个业务错误本身缺少 domain/application 类型，应在对应用例边界增加类型化错误，而不是直接在 HTTP adapter 中解析字符串。

## 4. 固定架构边界

错误模型必须遵守现有依赖方向：

~~~
Browser / Yew
      │
      │ HTTP error envelope
      ▼
hyz-things inbound HTTP
      │
      │ typed application errors
      ▼
hyz-things application
      │
      ├──────── camera port ────────> hyz-camera UDS
      │
      └──────── router port ────────> hyz-router UDS
~~~

约束：

- domain/application 错误不得依赖 Axum、HTTP status、Yew 或具体 Linux adapter。
- HTTP status 与 API error.code 的映射只属于 inbound HTTP 边界。
- hyz-contract 只承载 versioned wire value 和 transport 必需类型，不依赖 Axum/Yew。
- transport 错误与 remote application 错误必须可区分。
- outbound adapter 可以把 Linux/io/remote 错误映射为 application-level 类型，但不能把 HTTP DTO 反向带入 application。
- Web 继续只通过 HTTP API 获取错误，不依赖 native Rust error 类型。

## 5. 目标错误模型

### 5.1 三层语义

错误统一按三层处理：

~~~
domain / application error
        │
        │ boundary mapping
        ▼
stable service error classification
        │
        ├──────── structured log
        │
        └──────── HTTP / UDS representation
~~~

第一层描述真实业务和用例失败；第二层提供稳定 code 与错误类别；第三层负责具体协议表示。

不得直接用 Display 字符串作为第二层。

### 5.2 Stable error code

对外和日志共用的稳定 code 使用小写 snake_case，保持与现有 camera_* 风格一致。

code 表达“模块 + 可执行语义”，例如：

- auth_invalid_credentials
- auth_invalid_session
- auth_password_change_required
- auth_invalid_new_password
- auth_rate_limited
- auth_unavailable
- network_probe_failed
- network_command_failed
- network_conflict
- proxy_probe_failed
- proxy_command_failed
- subscription_refresh_failed
- device_policy_generation_conflict
- tailscale_status_unavailable
- tailscale_reconcile_failed
- tailscale_self_stop_forbidden
- camera_busy
- camera_not_ready

具体完整 code registry 在实施第一阶段由当前所有 HTTP/control 失败路径审计产生并写入测试。

规则：

- code 是程序判断依据，稳定性高于 message 文案。
- code 不包含动态数据、路径、MAC、IP、provider 名、session id、URL、command output 或 errno。
- 不为同一语义因为 endpoint 不同而随意创建多个 code。
- 不把所有内部失败细分成公开 code；对外只暴露调用方可以安全理解和处理的粒度。
- unknown/internal failure 可以映射为受控 generic code，但 generic code 不能继续替代已知业务分类。
- Camera 现有 code 默认保持不变。

### 5.3 Error class

稳定 code 之外建立有限的高层 class，用于协议映射和日志 severity，不代替业务 code。

建议类别：

- invalid_request
- unauthorized
- forbidden
- not_found
- conflict
- unavailable
- internal

HTTP status 主要由 class 决定，业务 code 决定具体语义。

例如：

~~~
device_policy_generation_conflict
  class = conflict
  HTTP = 409

tailscale_status_unavailable
  class = unavailable
  HTTP = 503

auth_invalid_credentials
  class = unauthorized
  HTTP = 401
~~~

现有特殊状态，例如 413 Payload Too Large、422 Unsupported Offer、429 Rate Limited，应保留显式映射，不为了强行塞入通用 class 而改变既有 HTTP 行为。

### 5.4 Public message 与 internal detail 分离

每个错误至少区分：

- stable code：机器可判断；
- public message：允许返回浏览器的安全描述；
- internal detail/source：仅供服务端日志与错误链使用。

不得把任意 source Display 直接作为 public message。

例如 command stderr、filesystem path、raw provider response、Tailscale auth URL 或 credential storage detail 只能进入经过审计的内部日志字段，不能直接返回浏览器。

## 6. hyz-router control 错误传播

### 6.1 保留现有 wire envelope

hyz-contract 当前 ControlError 已有：

~~~
code: String
message: String
~~~

首选方案是不改变该 JSON shape，因此不因为本计划本身提升 PROTOCOL_VERSION。

router server 不再把所有 ControlHandler failure 统一写成 operation_failed，而是由 application/inbound boundary 提供稳定 remote code。

如果实施中必须改变 wire shape 或字段语义达到不兼容程度，则必须按 AGENTS.md 的 protocol 规则单独评估版本提升；不得顺手修改版本。

### 6.2 server-side typed control failure

router side 的 ControlHandler 不应继续只返回 String。

应引入 application/server-facing 的 typed failure seam，例如概念上：

~~~
ControlFailure {
    code,
    safe_message,
    source/detail
}
~~~

具体 Rust 类型名允许调整。

业务 application error 在进入 control adapter 前映射成稳定 code；control adapter 只负责 serialization，不通过字符串匹配猜错误类型。

unsupported_version 与 invalid_request 继续作为协议层错误，不与业务错误混用。

### 6.3 contract client 区分 transport 和 remote failure

当前 hyz-contract client 把 ControlError 格式化进 std::io::Error，导致 remote code 丢失。

目标 client error 至少区分：

~~~
Transport / Protocol failure
Remote(ControlError)
~~~

调用方必须可以读取 remote code，而不是解析 error.to_string()。

transport failure 例如：

- connect timeout
- socket unavailable
- frame invalid
- response timeout
- protocol version mismatch

不能伪装成某个业务 remote code。

### 6.4 hyz-things PortalControlHandler 保留 typed remote error

PortalControlHandler 不再返回 Result<ControlResult, String>。

目标边界应保留：

- transport/protocol failure；
- remote stable code；
- safe remote message；
- 必要的 internal source。

RouterControlClient 负责把 hyz-contract client error 映射到 portal application boundary；HTTP adapter 再把该错误映射为 HTTP class/code。

这一层完成后，HTTP 不允许继续通过 message 字符串判断 conflict、unavailable 或业务失败。

## 7. hyz-things HTTP 错误 contract

### 7.1 统一 envelope

保持当前 error object 风格，目标形态为：

~~~
{
  "error": {
    "code": "device_policy_generation_conflict",
    "message": "Device policy changed; refresh and retry"
  }
}
~~~

可在不破坏现有客户端的前提下增加 request correlation 字段或 response header，但不得让客户端必须依赖它才能判断错误。

### 7.2 server-generated request ID

每个 HTTP request 由服务端生成 bounded opaque request_id：

- 不信任或直接复用浏览器传入的 request id；
- request_id 不编码用户信息、IP、session、时间戳明文或 secret；
- 同一 request 的 inbound、application/remote failure 和 response 日志使用同一 ID；
- error response 建议通过固定 response header 返回 request ID，便于人工排障；
- 前端可以展示或复制 request ID，但不得把它作为业务状态。

具体 header 名在实现阶段固定，并由 HTTP 测试覆盖。

### 7.3 HTTP status 与 code

原则：

- status 表达协议级大类；
- code 表达具体稳定语义；
- message 只做安全的人类描述。

现有已合理的状态尽量保持，例如：

- malformed/invalid request -> 400
- unauthenticated/invalid session -> 401
- authenticated but forbidden -> 403
- missing resource/session -> 404
- state/generation/busy conflict -> 409
- payload too large -> 413
- semantically unsupported camera offer -> 422
- rate limit -> 429
- dependency unavailable -> 503

不能为了获得更多 code 而把原本正确的 HTTP status 全部改成 500。

### 7.4 Generic code 的使用范围

以下 generic code 可以保留，但限制使用：

- invalid_request：仅用于无法进一步安全分类的 request validation failure；
- forbidden：仅用于真正通用的授权拒绝；
- not_found：仅用于没有更具体资源语义且公开细分无价值的场景；
- service_unavailable：仅用于无法确认更具体依赖类型的 infrastructure unavailable。

control_failed 不再作为已知 router/control 业务失败的默认汇聚点。

authentication_failed 不再覆盖所有 AdminError；按安全允许的稳定 auth code 映射。

## 8. Authentication 错误映射

AdminError 继续保持 application-level 语义，不依赖 HTTP。

HTTP 最低应区分：

- auth_invalid_credentials
- auth_invalid_session
- auth_invalid_new_password
- auth_password_change_required
- auth_rate_limited
- auth_unavailable

安全要求：

- 不泄漏 password hash、credential store path、Argon2 detail、session token 或 CSRF token；
- invalid credential message 不解释密码哪一部分错误；
- rate-limit 日志不得记录提交的 credential；
- session invalid/expired 可以作为同一 public code，如果 application 本身无法或不应安全区分；
- Web 不通过 message 文案识别“需要重新登录”。

## 9. Camera 错误保持与对齐

Camera HTTP 现有 code 作为参考基线，原则上不改名。

本计划只做：

- 将 Camera response envelope 与其他 API 的统一 helper 对齐；
- 增加 request_id / structured log 关联；
- 确认 CameraUnixAdapter 的 transport failure 与 CameraError::Unavailable 不会泄漏内部 detail；
- 为现有 camera code 增加 contract tests，防止后续被重新压成 generic code。

不得借本计划改变 Camera viewer token、session ownership、media lifecycle 或 fixed UDP pool。

## 10. 结构化日志

### 10.1 日志基础设施

hyz-things native side 引入统一 structured logging facade。

首选实现为 tracing + tracing-subscriber，并继续写 stderr，以保持现有 init/部署对进程输出的捕获方式。不得因为本计划引入外部日志服务。

生产日志至少能稳定表达 key/value 字段；具体 formatter 可根据目标板日志可读性选择 compact text 或 JSON，但字段 contract 必须由测试/集中 helper 固定，不允许每处自由拼接自然语言。

### 10.2 基础字段

关键 lifecycle / request / failure 日志至少考虑以下字段：

- service = hyz-things
- component
- operation
- event
- error_code
- request_id（HTTP request 内）
- remote_code（跨进程错误时，必要且安全）
- error_class
- duration_ms（适合 request/operation 时）
- result

不是每条日志都必须拥有所有字段，但 error_code 不得只藏在 message 中。

示例：

~~~
level=ERROR service=hyz-things component=tailscale_listener operation=reconcile event=operation_failed error_code=tailscale_reconcile_failed
~~~

HTTP remote control 失败示例：

~~~
level=WARN service=hyz-things component=http operation=device_policy_update request_id=... error_code=device_policy_generation_conflict remote_code=device_policy_generation_conflict
~~~

### 10.3 Event 与 message

event 使用低基数稳定值，例如：

- service_ready
- request_rejected
- dependency_unavailable
- operation_failed
- operation_completed
- reconcile_failed
- shutdown_failed

message 可以保留简短人类描述，但查询和测试不能依赖 message 精确文本。

### 10.4 Severity

避免所有可预期 4xx 都打 ERROR：

- 正常 lifecycle success：INFO；
- 可恢复/预期 conflict、rate limit、invalid request：INFO 或 WARN，按场景固定；
- dependency unavailable、reconcile failure：WARN/ERROR；
- 导致 request 5xx/503 的服务端失败：ERROR 或 WARN，按是否可预期恢复固定；
- 高频 polling failure 必须避免无界刷屏，必要时使用状态变化日志、采样或有界抑制。

具体 severity matrix 在实现第一阶段固定，避免同一 error_code 在不同位置随意使用不同级别。

## 11. 日志脱敏与隐私边界

普通日志严禁记录：

- administrator password / hash / salt；
- session cookie / session token；
- CSRF token；
- Camera viewer token；
- Camera session secret/token 部分；
- Tailscale auth key、node key、machine key；
- 完整 Tailscale login URL；
- subscription URL 中可能包含的 credential/token/query secret；
- raw HTTP Authorization/Cookie header；
- 完整 request body；
- raw provider configuration；
- 任意由浏览器提供的 shell/command 字符串（现有架构本来也不得存在）。

对于可能包含 secret 的 error source，不允许直接使用 debug dump 自动展开整个对象。

必须增加 negative tests，明确断言典型 secret fixture 不出现在日志输出和 HTTP error body 中。

## 12. 前端错误消费

apps/rust/things/src/web 只消费 HTTP error envelope。

前端规则：

- 已知行为根据 error.code 匹配；
- HTTP status 可用于大类 fallback；
- message 只作为最终展示文本或 unknown fallback；
- 不使用 contains、starts_with 或正则解析后端自然语言 message 来决定业务流程；
- 401/auth_invalid_session 触发现有认证失效流程；
- 409 generation/conflict 保留本地草稿并要求刷新，不自动重放危险 mutation；
- 503/unavailable 保留已有成功缓存并明确展示刷新失败；
- unknown code 必须保守展示 generic fallback，不崩溃。

如果当前前端已经存在 message-based branching，实施时必须逐项迁移并加测试。

## 13. 分阶段实施

### Phase 0：建立现状基线与错误路径清单

在修改行为前：

- 运行 make check-static；
- 运行 apps/rust/things 相关 native tests；
- 运行 strict Clippy；
- 运行 things HTTP tests；
- 运行 make things-e2e / Portal Playwright 可执行部分；
- 记录因环境缺失而未执行的项目，不能记为通过。

同时审计：

- hyz-things HTTP 所有 error response helper；
- AdminError / CameraError / PlatformError；
- PortalControlHandler；
- hyz-contract client；
- router ControlHandler 和 ControlResponse::error；
- Web 端所有 error.code / status / message 分支。

输出一份测试内可维护的 stable code registry，不额外建立第二份手工配置文件作为真相源。

### Phase 1：先固定 error taxonomy 与 HTTP contract tests

Red：

- 为已知不同错误写失败测试，证明当前会坍缩成同一个 control_failed / authentication_failed；
- 为 Camera 现有 code 写保持测试；
- 为 HTTP envelope 和 request correlation 写 contract test；
- 为 secret 不出现在 response/log 中写 negative test。

Green：

- 增加集中 error code/class 类型或 helper；
- 不先大规模改业务逻辑，只让新的 mapping seam 可以被测试。

Refactor：

- 清理 HTTP 中重复 Json error 构造；
- 保持 inbound HTTP adapter 不承担业务字符串解析。

### Phase 2：修复 router control typed error propagation

Red：

- router 两种不同 application failure 经 control socket 后应保留不同 code；
- remote error 与 transport error 必须可区分；
- hyz-things RouterControlClient 不得通过字符串解析获得 remote code。

Green：

- router ControlHandler 改为 typed failure seam；
- control adapter 序列化稳定 code；
- hyz-contract client 引入 typed client error；
- PortalControlHandler 保留 remote failure。

Refactor：

- 保持 ControlError wire shape 不变；
- 合并重复 remote/transport mapping；
- 确认没有 protocol shape change 时不提升版本。

### Phase 3：迁移 hyz-things HTTP mapping

按功能逐个 Red-Green-Refactor：

1. authentication；
2. network/Wi-Fi；
3. proxy/subscription；
4. device policy/activity；
5. Tailscale；
6. generic control/read paths；
7. Camera helper 对齐但保持 code。

每轮只迁移一组明确行为，避免一次替换所有 error helper 后难以确认回归来源。

### Phase 4：引入 structured logging

Red：

- 捕获测试 subscriber/output，断言关键事件存在稳定字段；
- 断言 error_code/component/operation 不依赖 message；
- 断言 secret fixture 未出现在日志；
- 断言 request_id 能在 request failure 日志与 response header 间关联。

Green：

- composition root 初始化 tracing subscriber；
- 替换 main.rs 中现有 eprintln!；
- HTTP failure boundary 增加统一 structured event；
- Tailscale listener lifecycle/reconcile 使用稳定 event/code。

Refactor：

- 提取集中 logging helper/span；
- 避免每个 handler 手写重复字段；
- 对高频 polling failure 做必要的有界降噪。

### Phase 5：前端切换到 stable code

Red：

- 为认证过期、generation conflict、service unavailable 等已有重要流程写 Web/E2E 测试；
- 测试后端 message 文案变化不应改变前端行为。

Green：

- API client 解析统一 error envelope；
- 页面使用 code/status 做 typed branching；
- 保留 unknown fallback。

Refactor：

- 删除 message substring 判断；
- 收敛重复错误展示逻辑。

### Phase 6：清理 legacy generic paths

完成所有受支持路径后：

- 搜索并审计 control_failed；
- 搜索并审计 authentication_failed；
- 搜索 production eprintln!/println!；
- 搜索对 error message 的 contains/starts_with/字符串匹配；
- 搜索把 ControlError 直接格式化成普通 io::Error 的路径。

只有真正 generic 的场景允许保留 generic code，并在代码注释/测试中说明原因。

## 14. TDD 与测试矩阵

必须遵守 AGENTS.md 的 Red-Green-Refactor。

### 14.1 Unit / application tests

至少覆盖：

- 每个 domain/application error 映射到预期 stable code/class；
- 同一语义在不同调用路径保持同一 code；
- public message 不包含 internal detail；
- unknown source 保守映射；
- secret-bearing source 被脱敏。

### 14.2 hyz-contract / router control tests

至少覆盖：

- unsupported_version；
- invalid_request；
- 两个不同业务 remote error code；
- transport timeout 与 remote error 可区分；
- current/previous protocol compatibility 规则不因错误改造被破坏；
- ControlError payload 仍受 MAX_FRAME_BYTES 限制；
- secret 不进入 ControlError message。

### 14.3 HTTP tests

至少覆盖：

- status + code 对应关系；
- envelope shape；
- request ID header/日志关联；
- 400/401/403/404/409/413/422/429/503 关键路径；
- Camera code 保持；
- router remote code 不再统一为 control_failed；
- auth error 不再全部统一为 authentication_failed；
- unknown remote code 安全 fallback；
- API fallback/method-not-allowed 行为不被破坏。

### 14.4 Structured logging tests

至少覆盖：

- service/component/operation/error_code 字段；
- Tailscale status/reconcile failure；
- HTTP request failure；
- remote router failure；
- service_ready；
- shutdown/cleanup failure；
- password/session/CSRF/viewer token/login URL/subscription secret 不出现在输出中。

### 14.5 Web/E2E

至少覆盖：

- 401 session invalid -> 登录状态失效；
- 409 generation conflict -> 保留草稿、刷新服务器状态；
- 503 -> 保留旧数据并显示刷新失败；
- Camera busy/not ready 使用现有 code；
- 后端 message 文案改变时上述行为保持不变；
- unknown code 显示安全 fallback。

## 15. 兼容性与迁移规则

### 15.1 HTTP

error object 的 code 从过宽 generic 值迁移为更具体值属于语义变化。

实施前必须审计仓库内所有消费者；仓库内 Web 与 E2E 必须同一 PR 更新。

如果存在仓库外稳定客户端依赖旧 generic code，应在实现 PR 中明确兼容策略；不能默认外部客户端不存在。

### 15.2 Router control protocol

优先保持 ControlError 的 wire shape：

~~~
{ "code": "...", "message": "..." }
~~~

只改变具体 code 值和 client-side typed transport API，不主动修改 frame shape。

若 wire shape 不变并且 current/previous 版本客户端都能安全接受新的 String code，则不提升 protocol version。

若实施发现必须改变 serialization shape、required field 或版本兼容语义，则停止该阶段，单独评估协议版本，不在重构中隐式破坏兼容。

### 15.3 日志

日志 message 文案不视为兼容 contract；稳定字段名和 error_code 才是新的诊断 contract。

不得承诺所有 debug/internal 字段长期稳定，只有明确列入 plan/test 的核心字段稳定。

## 16. 实施文件范围预估

可能涉及但不限于：

- apps/rust/things/Cargo.toml
- apps/rust/things/src/main.rs
- apps/rust/things/src/application/ports.rs
- apps/rust/things/src/application/admin.rs
- apps/rust/things/src/application/camera.rs
- apps/rust/things/src/adapters/inbound/http/mod.rs
- apps/rust/things/src/adapters/outbound/router.rs
- apps/rust/things/src/adapters/outbound/camera.rs
- apps/rust/things/src/adapters/outbound/tailscale.rs
- apps/rust/things/src/web/api/*
- apps/rust/things/src/web/pages/*
- apps/rust/things/tests/http_status.rs
- apps/rust/contract/src/router.rs
- apps/rust/contract/src/client.rs
- apps/rust/router/src/application/*
- apps/rust/router/src/adapters/inbound/control.rs
- 对应 router/things contract、unit、HTTP、E2E tests

不应因为本计划修改与错误传播无关的 router outbound platform、camera media pipeline 或前端视觉结构。

## 17. 分支与 PR 策略

实现使用独立分支，例如：

~~~
refactor/hyz-things-error-model-logging
~~~

优先一个 PR 完成整个 error propagation contract，避免中间 main 出现“router 发新 code、things 仍丢失”或“前端依赖新 code、后端尚未提供”的不一致状态。

PR 内按有意义 checkpoint 批量提交和触发 CI，不为每个机械替换单独 push。

建议 checkpoint：

1. error contract tests + taxonomy seam；
2. router/contract typed propagation；
3. HTTP mappings；
4. structured logging；
5. Web consumer + E2E；
6. legacy cleanup + final validation。

最终候选 HEAD 必须完整通过相关 CI。

## 18. 完成标准

只有同时满足以下条件，计划才能标记“代码已实施”：

- router 已知 application failure 不再全部变成 operation_failed；
- hyz-contract client 能区分 transport/protocol failure 与 remote ControlError；
- PortalControlHandler 不再使用 String 作为唯一错误语义；
- hyz-things 已知控制失败不再全部变成 control_failed；
- AdminError 不再全部变成 authentication_failed；
- Camera 现有稳定 code 未回退；
- Web 的关键错误流程根据 code/status 判断，不解析 message；
- production hyz-things 关键生命周期/失败日志使用结构化字段，不再依赖裸 eprintln! 作为主要业务日志；
- HTTP request failure 可通过 server-generated request ID 与日志关联；
- 结构化日志至少稳定提供 service/component/operation/error_code 等核心字段；
- secret negative tests 通过；
- 所有相关 unit/contract/HTTP/E2E tests 通过；
- strict Clippy、静态检查和相关 GitHub Actions 在最终候选 HEAD 通过。

真实目标板验收至少确认：

- 服务启动/ready 日志正常；
- router unavailable / Tailscale status unavailable 等真实故障可以通过 error_code 明确定位；
- 日志中没有 admin/session/CSRF/Tailscale login URL 等敏感值；
- Web 仍能正确显示错误和恢复状态。

目标板验收未执行时，只能标记“代码已实施，目标板验收待完成”，不得写成全部完成。
