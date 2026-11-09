# 宿主机 OCR 与临时图片接收端

这两个服务只运行在**运行 Codex App Server 的电脑**上，由普通用户通过
`systemd --user` 托管：

- `hyz-paddleocr.service`：PaddleX OCR，仅监听 `127.0.0.1:8080`，供
  `hyz-image-ocr` Skill 调用。
- `hyz-image-receiver.service`：接收 RK3568 things 后端转发的临时图片，只在
  项目内 `tmp/hyz-ai-uploads/` 落盘一次，供 Codex `localImage` 与 OCR 读取。

设备端不会写图片到自己的磁盘，也不依赖 SMB/CIFS。

## 一、安装 PaddleOCR

使用非 root 用户执行：

```bash
mkdir -p ~/.local/share/hyz-ocr
python3 -m venv ~/.local/share/hyz-ocr/venv
# 如果系统 Python 缺少 ensurepip，可改用：
# uv venv --seed ~/.local/share/hyz-ocr/venv
~/.local/share/hyz-ocr/venv/bin/python -m pip install --upgrade pip

# CPU 版 PaddlePaddle；按官方说明选择与 Python 版本兼容的 wheel。
~/.local/share/hyz-ocr/venv/bin/python -m pip install paddlepaddle
~/.local/share/hyz-ocr/venv/bin/python -m pip install "paddleocr" "paddlex"

# PaddleX 3.7 的 serving 依赖需要单独安装，仍在 venv 内。
~/.local/share/hyz-ocr/venv/bin/paddlex --install serving
```

验证：

```bash
~/.local/share/hyz-ocr/venv/bin/python -c "import paddle, paddleocr; print(paddle.__version__)"
~/.local/share/hyz-ocr/venv/bin/paddlex --help
```

首次启动会下载 OCR 模型（约 180 MB），需要可用的网络与磁盘空间。模型缓存
位于 `~/.paddlex/official_models`。若 Hugging Face 不可达，使用单元里的
`PADDLE_PDX_MODEL_SOURCE=modelscope`。

当前 CPU wheel 上 `PP-OCRv6_medium_det` 的 oneDNN 路径会报
`ConvertPirAttribute2RuntimeAttribute not support`，单元已用
`PADDLE_PDX_ENABLE_MKLDNN_BYDEFAULT=False` 切回 paddle 运行模式，避免 OCR
接口返回 500。这是绕过上游后端缺陷，不影响识别结果。

## 二、启用 PaddleOCR 用户服务

```bash
mkdir -p ~/.config/systemd/user
cp scripts/ocr-host/hyz-paddleocr.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now hyz-paddleocr.service
systemctl --user status hyz-paddleocr.service
curl -fsS -X POST http://127.0.0.1:8080/ocr \
  -H 'Content-Type: application/json' \
  -d '{"file":"<base64>","fileType":1,"visualize":false}'
```

返回结构为 `result.ocrResults[].prunedResult.rec_texts`；`errorCode` 非 0 时
表示服务端错误。`hyz-image-ocr` 的脚本已处理该结构。

如果只有登录后才需要 OCR，无需其他设置；若希望未登录也自动启动，管理员可
执行一次：

```bash
sudo loginctl enable-linger "$USER"
```

OCR 服务固定监听 loopback，**不要把 0.0.0.0:8080 暴露到 LAN**。

## 三、启用临时图片接收端

不再需要 env 文件。unit 已把监听地址写死为本机 LAN：

```ini
Environment=HYZ_IMAGE_RECEIVER_BIND=192.168.8.202:8099
```

如果这台电脑的 LAN 地址变化，只改 unit 里的这一行。TTL（6 小时）和总磁盘
配额（512 MiB）使用代码内默认值。

接收端不使用令牌或 IP 白名单：上传入口只能由已登录管理员的 things 后端触发，
并继续受管理员 Session、同源、CSRF、图片校验与配额保护。仍建议用防火墙把
监听端口限制在 LAN。

```bash
mkdir -p ~/.config/systemd/user
cp scripts/ocr-host/hyz-image-receiver.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now hyz-image-receiver.service
systemctl --user status hyz-image-receiver.service
curl -fsS http://192.168.8.202:8099/health
```

接收端只写 `<项目根>/tmp/hyz-ai-uploads/`，目录权限 `0700`、文件权限
`0600`。文件按 TTL 清理；项目根 `tmp/` 已在 `.gitignore` 中，不会进入提交。
`tmp/` 会跨重启保留，因此清理依赖 TTL 与配额，不依赖系统临时目录策略。

### 防火墙示例

可选：只允许 RK3568 访问接收端口（示例，按实际网段调整）：

```bash
sudo nft add rule inet filter input ip saddr != 192.168.8.100 tcp dport 8099 drop
```

或 iptables：

```bash
sudo iptables -A INPUT -p tcp --dport 8099 ! -s 192.168.8.100 -j DROP
```

## 四、RK3568 配置

不需要额外配置，也不需要 upload URL env：things 后端使用固定地址
`http://192.168.8.202:8099`。该地址不可由浏览器输入、AI 生成或 env 覆盖。
接收端未运行时，AI 解析仍可进行纯文字对话。

## 五、诊断

```bash
systemctl --user status hyz-paddleocr.service hyz-image-receiver.service
journalctl --user -u hyz-paddleocr.service -n 100 --no-pager
journalctl --user -u hyz-image-receiver.service -n 100 --no-pager
ls -ld ~/Code/hyz_things/tmp/hyz-ai-uploads
```

- OCR 服务不可用：确认 `curl` 的 OCR 请求成功；首次模型下载可能较慢。AI
  仍应能按原图继续解读，并在需要时报告歧义。
- 图片上传 503：检查接收端服务、`192.168.8.202:8099` 可达性和防火墙。
- 上传 400/413/415：检查文件是否为可读 JPG/PNG、是否超过 8 MiB 或尺寸上限。
- 上传 507：清理 `tmp/hyz-ai-uploads/` 中过期文件或提高配额。

## 六、卸载

```bash
systemctl --user disable --now hyz-paddleocr.service hyz-image-receiver.service
rm -f ~/.config/systemd/user/hyz-paddleocr.service \
      ~/.config/systemd/user/hyz-image-receiver.service
systemctl --user daemon-reload
# 确认不再需要模型和临时文件后，可手动删除：
# rm -rf ~/.local/share/hyz-ocr ~/.paddlex ~/.cache/paddle ~/Code/hyz_things/tmp/hyz-ai-uploads
```
