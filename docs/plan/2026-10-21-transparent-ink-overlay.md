# 专项训练透明手写草稿层替换

> 日期：2026-10-21
> 状态：用户确认直接替换原独立黑色草稿区；不提供双模式。

## 用户目标

在「专项训练 → 速算训练 / 错题复练」的题目与选项上叠加透明 Canvas，模仿粉笔 APP 的铅笔入口和撤销、重做、铅笔、清屏、关闭工具栏。关闭草稿立即恢复题目滚动和选项交互，切题自动清空。Flashcard 记忆复习保持不变。

## 实现约束

- 只在专项训练中复用透明覆盖组件；不得更改 FSRS、Flashcard Markdown、后台 API 或 SQLite。
- Canvas 内坐标继续使用 900×450，原始采样继续 pointerdown/pointermove 的直接 lineTo，无平滑、无压感；线宽 3.8、圆头、圆角；在浅色题目上改为高对比玫红色笔迹，避免原白色笔迹不可见。
- 保留 iPad Safari passive:false touchstart/touchend、防止快速连续 Pencil 落笔被吞、pen 不主动 pointer capture、前一 pen pointerup 遗失由下次 pen pointerdown 接管。
- 浏览器内存保存每笔轨迹；撤销/重做按笔画，清屏也作为可撤销动作。直接 pointermove 绘制，不在每个采样点触发 Yew state render。新落笔使 redo 失效。
- Canvas 开启时在题面上透明覆盖，不可误点下层选项；关闭后可正常点击选项、滚动卡片。默认关闭，切题清空并关闭，单题内反复关闭/开启不清掉已写的笔。
- 删除原黑底独立草稿区域及宽屏左右双列布局，保持题目区单列可手写。
- 不含图片导出、草稿持久化、OCR、AI 判卷或橡皮擦。

## 验收

- [ ] 速算及错题复练均可点铅笔展开透明草稿，关闭后答题交互正常。
- [ ] 撤销、重做、清屏及撤销清屏都可使用；清空后画布与新题空白一致。
- [ ] Pencil 在 Safari 连续落笔、丢失 pointerup 时能继续写。
- [ ] 题目仍可读取 Markdown/LaTeX，FSRS 不记录错题复练自评。
- [ ] Rust 单测 / WASM 构建 / Playwright E2E / Portal CI 通过。
