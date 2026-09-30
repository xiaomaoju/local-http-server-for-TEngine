const editableSelector = "input, textarea, [contenteditable]";

function disableTextAssistance(element: Element) {
  element.setAttribute("autocapitalize", "off");
  element.setAttribute("autocorrect", "off");
  element.setAttribute("spellcheck", "false");
}

export function installInputDefaults() {
  function apply(root: ParentNode) {
    if (root instanceof Element && root.matches(editableSelector))
      disableTextAssistance(root);
    root.querySelectorAll(editableSelector).forEach(disableTextAssistance);
  }

  apply(document);
  // Includes inputs opened later in dialogs, drawers and either desktop mode.
  const observer = new MutationObserver((records) => {
    for (const record of records)
      for (const node of record.addedNodes)
        if (node instanceof Element) apply(node);
  });
  observer.observe(document.body, { childList: true, subtree: true });
  return () => observer.disconnect();
}
