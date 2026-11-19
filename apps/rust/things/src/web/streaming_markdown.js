const sourceByBlock = new WeakMap();

function blockSource(node) {
  return node.nodeType === Node.ELEMENT_NODE ? node.outerHTML : node.textContent;
}

export function updateStreamingMarkdown(root, html) {
  const template = document.createElement('template');
  template.innerHTML = html;
  const nodes = Array.from(template.content.childNodes).filter(node =>
    node.nodeType !== Node.TEXT_NODE || node.textContent.trim() !== ''
  );
  // Diagram and math hydration can mutate block contents after the source is recorded.
  let index = 0;
  while (index < nodes.length && index < root.children.length) {
    if (sourceByBlock.get(root.children[index]) !== blockSource(nodes[index])) break;
    index++;
  }
  while (root.children.length > index) root.lastElementChild.remove();
  for (; index < nodes.length; index++) {
    const node = nodes[index];
    const block = document.createElement('div');
    sourceByBlock.set(block, blockSource(node));
    block.append(node);
    root.append(block);
  }
}
