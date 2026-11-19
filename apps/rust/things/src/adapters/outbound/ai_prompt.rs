//! Shared output contract for AI explanation prompts: Markdown body plus
//! optional safe SVG diagrams. The browser validates every diagram before it
//! replaces the code block with an image.

pub(crate) const AI_REVIEW_ENVELOPE: &str =
    "输出格式：直接输出 Markdown 正文，不要用 JSON 包装或用代码围栏包裹全文。\n";

pub(crate) const AI_SVG_INSTRUCTIONS: &str = concat!(
    "当图形有助理解时，用独立的 ```svg 代码块绘制线段图、数轴、对比图或流程图；\n",
    "图形应简洁并有中文文字标注。根元素示例：<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 600 240\">。\n",
    "可使用渐变、样式、变换、滤镜及 #id 内部引用，保持 SVG 结构简洁。\n",
    "禁止脚本、事件处理器、foreignObject、外部资源和外部引用；每图最多 256 个元素、源码不超过 32768 字符。\n",
    "图形显示在白色背景上，请使用深色文字和线条。无需画图时直接使用文字、表格或公式，不必强制绘图。\n",
);
