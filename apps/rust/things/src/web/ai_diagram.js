export function renderAiDiagrams(root, validate) {
  if (!root) return;
  root.querySelectorAll('[data-ai-diagram-source="true"] pre > code.language-svg').forEach(code => {
    const pre = code.parentElement;
    if (!pre || pre.dataset.aiDiagramChecked) return;
    pre.dataset.aiDiagramChecked = 'true';
    let source = null;
    try { source = code.textContent || ''; if (!validate(source)) source = null; } catch (_) { /* Keep the escaped source visible. */ }
    if (!source) {
      const warning = document.createElement('p');
      warning.textContent = '此 SVG 不符合安全绘图格式，已保留源码。';
      warning.className = 'ai-diagram-warning';
      warning.dataset.testid = 'question-bank-ai-diagram-error';
      pre.before(warning);
      return;
    }
    const figure = document.createElement('figure');
    figure.className = 'ai-diagram';
    const image = document.createElement('img');
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(source)}`;
    image.alt = 'AI 解析示意图';
    image.dataset.testid = 'question-bank-ai-diagram';
    figure.append(image);
    pre.replaceWith(figure);
  });
}
