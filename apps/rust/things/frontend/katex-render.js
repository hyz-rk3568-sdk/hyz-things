window.hyzRenderMath = function (root) {
  if (!window.katex || !root) return;
  root.querySelectorAll('.math-inline, .math-display').forEach(function (element) {
    if (element.dataset.katexRendered === 'true') return;
    try {
      window.katex.render(element.textContent || '', element, {
        displayMode: element.classList.contains('math-display'),
        throwOnError: false,
        trust: false,
        strict: 'warn'
      });
      element.dataset.katexRendered = 'true';
    } catch (_) {
      element.dataset.katexRendered = 'error';
    }
  });
};
