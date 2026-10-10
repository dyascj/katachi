// Renames the selected layers (or every top-level layer) after their type and position.
const nodes = figma.currentPage.selection.length ? figma.currentPage.selection : figma.currentPage.children;
const sorted = [...nodes].sort((a, b) => a.y - b.y || a.x - b.x);
sorted.forEach((n, i) => {
  const type = n.type[0] + n.type.slice(1).toLowerCase().replace(/_/g, " ");
  n.name = `${type} ${i + 1}`;
});
figma.closePlugin(`Renamed ${sorted.length} layers`);
