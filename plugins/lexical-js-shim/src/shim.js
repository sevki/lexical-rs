// Run Lexical for JavaScript behind a document-in / document-out boundary.
//
// Each call builds a headless Lexical editor, loads the document the host sent, restores
// the selection, runs the command through the registered plugins, flushes the pending
// commit (update listeners such as markdown shortcuts run then) and returns the document.

import {createHeadlessEditor} from '@lexical/headless';
import {
  $getRoot,
  $getSelection,
  $isRangeSelection,
  $isElementNode,
  $isTextNode,
  $isLineBreakNode,
  $createRangeSelection,
  $setSelection,
  CONTROLLED_TEXT_INSERTION_COMMAND,
  INSERT_PARAGRAPH_COMMAND,
  DELETE_CHARACTER_COMMAND,
  FORMAT_TEXT_COMMAND,
} from 'lexical';
import {nodes, register} from './plugins.js';

/** WIT command names (see `document-plugin.run`) to Lexical commands. */
const COMMANDS = {
  'insert-text': (payload) => [CONTROLLED_TEXT_INSERTION_COMMAND, payload],
  'insert-paragraph': () => [INSERT_PARAGRAPH_COMMAND, undefined],
  'delete-backward': () => [DELETE_CHARACTER_COMMAND, true],
  'delete-forward': () => [DELETE_CHARACTER_COMMAND, false],
  'format-text': (payload) => [FORMAT_TEXT_COMMAND, payload],
};

function makeEditor() {
  const editor = createHeadlessEditor({
    namespace: 'lexical-rs',
    nodes,
    onError: (error) => {
      throw error;
    },
  });
  register(editor);
  return editor;
}

// A "line block" holds inline content: not the root and not a list wrapper. The host
// counts blocks the same way, which is how positions are exchanged.
function lineBlocks() {
  const out = [];
  const walk = (element) => {
    for (const child of element.getChildren()) {
      if (!$isElementNode(child) || child.isInline()) continue;
      const holdsBlocks = child.getChildren().some((c) => $isElementNode(c) && !c.isInline());
      if (holdsBlocks) walk(child);
      else out.push(child);
    }
  };
  walk($getRoot());
  return out;
}

function inlineLength(node) {
  if ($isTextNode(node)) return node.getTextContentSize();
  if ($isLineBreakNode(node)) return 1;
  if ($isElementNode(node)) return node.getChildren().reduce((n, c) => n + inlineLength(c), 0);
  return 0;
}

function pointAt(block, offset) {
  let rest = offset;
  const find = (element) => {
    for (const child of element.getChildren()) {
      if ($isTextNode(child)) {
        const length = child.getTextContentSize();
        if (rest <= length) return {key: child.getKey(), offset: rest, type: 'text'};
        rest -= length;
      } else if ($isLineBreakNode(child)) {
        rest -= 1;
      } else if ($isElementNode(child)) {
        const hit = find(child);
        if (hit) return hit;
      }
    }
    return null;
  };
  return find(block) ?? {key: block.getKey(), offset: 0, type: 'element'};
}

function offsetOf(block, point) {
  const target = point.getNode();
  if (target.is(block)) return 0;
  let total = 0;
  let found = false;
  const walk = (element) => {
    for (const child of element.getChildren()) {
      if (found) return;
      if (child.is(target)) {
        found = true;
        total += point.type === 'text' ? point.offset : 0;
        return;
      }
      if ($isElementNode(child)) walk(child);
      else total += inlineLength(child);
    }
  };
  walk(block);
  return total;
}

function restoreSelection(selection) {
  const blocks = lineBlocks();
  const at = (position) => pointAt(blocks[Math.min(position.block, blocks.length - 1)], position.offset);
  const anchor = at(selection.anchor);
  const focus = at(selection.focus);
  const range = $createRangeSelection();
  range.anchor.set(anchor.key, anchor.offset, anchor.type);
  range.focus.set(focus.key, focus.offset, focus.type);
  $setSelection(range);
}

function currentSelection() {
  const selection = $getSelection();
  if (!$isRangeSelection(selection)) return null;
  const blocks = lineBlocks();
  const position = (point) => {
    const node = point.getNode();
    const index = Math.max(
      blocks.findIndex((b) => b.is(node) || node.getParents().some((p) => p.is(b))),
      0,
    );
    return {block: index, offset: offsetOf(blocks[index], point)};
  };
  return {anchor: position(selection.anchor), focus: position(selection.focus)};
}

export function run(stateJson, selection, name, payload) {
  const editor = makeEditor();
  editor.setEditorState(editor.parseEditorState(stateJson), {tag: 'history-merge'});
  if (selection) editor.update(() => restoreSelection(selection), {discrete: true});

  let handled = false;
  const command = COMMANDS[name];
  if (command) {
    const [type, argument] = command(payload);
    handled = editor.dispatchCommand(type, argument);
    // Commits are normally deferred to a microtask. Flush them now, repeating while
    // listeners schedule follow-up updates.
    for (let i = 0; i < 4; i++) editor.update(() => {}, {discrete: true});
  }

  let out = null;
  editor.read(() => {
    out = currentSelection();
  });
  return {state: JSON.stringify(editor.getEditorState().toJSON()), selection: out, handled};
}
