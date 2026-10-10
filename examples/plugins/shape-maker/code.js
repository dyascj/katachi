// A plugin with a UI: the panel asks for shapes, the plugin makes them and reports back.
figma.showUI(__html__, { width: 260, height: 210 });
const report = () => figma.ui.postMessage({ type: "selection", count: figma.currentPage.selection.length });
figma.on("selectionchange", report);
report();
figma.ui.onmessage = (msg) => {
  if (msg.type === "create") {
    const made = [];
    for (let i = 0; i < msg.count; i++) {
      const s = msg.shape === "ellipse" ? figma.createEllipse() : figma.createRectangle();
      s.x = i * 120;
      s.fills = [{ type: "SOLID", color: { r: 0.56, g: 0.45, b: 1 } }];
      made.push(s);
    }
    figma.currentPage.selection = made;
    figma.notify(`Made ${made.length} shapes`);
  }
  if (msg.type === "close") figma.closePlugin();
};
