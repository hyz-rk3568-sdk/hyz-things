---
name: hyz-image-ocr
description: 在 hyz-things 真题 AI 解析中，对用户本次上传的手写、数字、分数线、上下标或印刷模糊图片按需调用宿主机 127.0.0.1:8080 的本地 PaddleOCR，作为对照原图的辅助证据；不安装或启动服务，不访问任意路径。
---

# hyz-image-ocr

## 何时使用

- 先直接查看用户本次消息附带的原图。只有数字、运算符、分数线、上下标、圈画或中文手写存在歧义，或需要逐字核对大段印刷文字时，才调用本 Skill。
- 不要对清晰可读的图片重复 OCR，也不要用 OCR 输出替代对原图的判断。
- OCR 是辅助证据：必须回到原图核对；用户速算过程、合理近似和不确定符号不擅自改写。若两者冲突，明确报告歧义。
- 普通 OCR 不等同于数学公式识别；不要假定它能稳定生成 LaTeX。

## 调用方式

只接受本次上传的原图，路径必须位于当前项目：

```text
<项目根>/tmp/hyz-ai-uploads/<uuid>.png
<项目根>/tmp/hyz-ai-uploads/<uuid>.jpg
```

在该项目根目录执行：

```bash
python3 .agents/skills/hyz-image-ocr/scripts/ocr_client.py \
  tmp/hyz-ai-uploads/<uuid>.png
```

- 成功时输出 JSON：`{"ok": true, "lines": [...], "raw": ...}`；`lines` 是识别文本，`raw` 供排查。
- 失败时输出 `{"ok": false, "error": "..."}` 并以非零状态退出；此时直接说明 OCR 不可用，继续按原图给出不确定标注。
- 脚本固定请求本机 `http://127.0.0.1:8080/ocr`，可用 `HYZ_OCR_URL` 覆盖为同机 loopback 端口。它不访问网络、不执行图片内容，也不接受外部 URL。
- 不要读取 `tmp/hyz-ai-uploads/` 以外的文件；不要尝试安装依赖、启动服务、执行 sudo、修改服务配置或把原始图片复制到别处。

## 维护与诊断

宿主机安装、systemd --user 单元和卸载步骤见 `scripts/ocr-host/README.md`。服务不可用时只报告状态，不自行修复。
