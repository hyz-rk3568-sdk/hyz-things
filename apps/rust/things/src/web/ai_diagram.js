const SVG_NS = 'http://www.w3.org/2000/svg';
const XMLNS_NS = 'http://www.w3.org/2000/xmlns/';
const MAX_SVG_LENGTH = 32_768;
const MAX_ELEMENTS = 256;
const MAX_DEPTH = 16;
const ELEMENTS = new Set(['svg', 'g', 'path', 'line', 'polyline', 'polygon', 'rect', 'circle', 'ellipse', 'text', 'tspan', 'title', 'desc']);
const NUMERIC = new Set(['x', 'y', 'x1', 'y1', 'x2', 'y2', 'cx', 'cy', 'r', 'rx', 'ry', 'width', 'height', 'stroke-width', 'font-size', 'opacity', 'fill-opacity', 'stroke-opacity']);
const COLORS = new Set(['fill', 'stroke']);
const CHOICES = {
  'text-anchor': new Set(['start', 'middle', 'end']),
  'dominant-baseline': new Set(['auto', 'middle', 'central', 'hanging', 'alphabetic']),
  'stroke-linecap': new Set(['butt', 'round', 'square']),
  'stroke-linejoin': new Set(['miter', 'round', 'bevel']),
  'font-weight': new Set(['normal', 'bold', '100', '200', '300', '400', '500', '600', '700', '800', '900']),
  'font-family': new Set(['sans-serif', 'serif', 'monospace']),
};
const NUMBER = /^[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?$/;
const NUMBERS = /[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?/g;

function boundedNumber(value) {
  return NUMBER.test(value) && Number.isFinite(Number(value)) && Math.abs(Number(value)) <= 10_000;
}

function validList(value, allowed) {
  if (!value || value.length > 8192 || !allowed.test(value)) return false;
  const numbers = value.match(NUMBERS) || [];
  return numbers.length > 0 && numbers.length <= 2048 && numbers.every(boundedNumber);
}

function validAttribute(element, attribute) {
  const { name, value, namespaceURI } = attribute;
  if (name === 'xmlns') return element.localName === 'svg' && namespaceURI === XMLNS_NS && value === SVG_NS;
  if (namespaceURI) return false;
  if (name === 'viewBox') {
    if (element.localName !== 'svg' || !validList(value, /^[\d\s,\.eE+\-]+$/)) return false;
    const parts = value.match(NUMBERS) || [];
    return parts.length === 4 && Number(parts[2]) > 0 && Number(parts[3]) > 0;
  }
  if (NUMERIC.has(name)) return boundedNumber(value);
  if (COLORS.has(name)) return /^(?:none|currentColor|transparent|#[0-9a-f]{3,8}|[a-z]{3,20})$/i.test(value);
  if (Object.prototype.hasOwnProperty.call(CHOICES, name)) return CHOICES[name].has(value);
  if (name === 'd') return element.localName === 'path' && validList(value, /^[MmLlHhVvCcSsQqTtAaZz\d\s,\.eE+\-]+$/);
  if (name === 'points') return ['polygon', 'polyline'].includes(element.localName) && validList(value, /^[\d\s,\.eE+\-]+$/);
  return false;
}

function validateSvg(source) {
  if (!source || source.length > MAX_SVG_LENGTH || /<!\s*(?:doctype|entity)|<\?/i.test(source)) return null;
  const document = new DOMParser().parseFromString(source, 'image/svg+xml');
  if (document.doctype || document.querySelector('parsererror')) return null;
  const root = document.documentElement;
  if (!root || root.namespaceURI !== SVG_NS || root.localName !== 'svg' || !root.hasAttribute('viewBox')) return null;
  let count = 0;
  const walk = (element, depth) => {
    if (++count > MAX_ELEMENTS || depth > MAX_DEPTH || element.namespaceURI !== SVG_NS || !ELEMENTS.has(element.localName)) return false;
    if (element !== root && element.localName === 'svg') return false;
    for (const attribute of element.attributes) if (!validAttribute(element, attribute)) return false;
    for (const node of element.childNodes) {
      if (node.nodeType === Node.ELEMENT_NODE) {
        if (!walk(node, depth + 1)) return false;
      } else if (node.nodeType !== Node.TEXT_NODE && node.nodeType !== Node.COMMENT_NODE) {
        return false;
      }
    }
    return true;
  };
  return walk(root, 0) ? new XMLSerializer().serializeToString(root) : null;
}

export function renderAiDiagrams(root) {
  if (!root) return;
  root.querySelectorAll('[data-ai-diagram-source="true"] pre > code.language-svg').forEach(code => {
    const pre = code.parentElement;
    if (!pre || pre.dataset.aiDiagramChecked) return;
    pre.dataset.aiDiagramChecked = 'true';
    let source = null;
    try { source = validateSvg(code.textContent || ''); } catch (_) { /* Keep the escaped source visible. */ }
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
