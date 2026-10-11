// Yjs interop between Lexical for JavaScript (@lexical/yjs, binding v1) and lexical-yjs.
//
//   node yjs-interop.mjs gen              print {update, json} for a document Lexical wrote
//   node yjs-interop.mjs load FILE        load the hex Yjs update in FILE, print Lexical's JSON
//   node yjs-interop.mjs edit FILE        load FILE, append " js" to the first text node the way
//                                         a user would, print {update, json} (update = full state)
import * as Y from 'yjs';
import {createHeadlessEditor} from '@lexical/headless';
import {createBinding, syncLexicalUpdateToYjs, syncYjsChangesToLexical} from '@lexical/yjs';
import {$getRoot, $createParagraphNode, $createTextNode, $createLineBreakNode, $isTextNode, $isElementNode} from 'lexical';
import {HeadingNode, QuoteNode, $createHeadingNode, $createQuoteNode} from '@lexical/rich-text';
import {ListNode, ListItemNode, $createListNode, $createListItemNode} from '@lexical/list';
import {LinkNode, AutoLinkNode, $createLinkNode} from '@lexical/link';
import {CodeNode, $createCodeNode} from '@lexical/code';
import {readFileSync} from 'node:fs';

const nodes = [HeadingNode, QuoteNode, ListNode, ListItemNode, LinkNode, AutoLinkNode, CodeNode];
const hex = (u) => Buffer.from(u).toString('hex');
const unhex = (s) => Uint8Array.from(Buffer.from(s.trim(), 'hex'));

function bound(doc) {
  const editor = createHeadlessEditor({nodes, onError: (e) => { throw e; }});
  const awareness = {getLocalState: () => null, getStates: () => new Map(), off() {}, on() {}, setLocalState() {}, setLocalStateField() {}};
  const provider = {awareness, connect() {}, disconnect() {}, on() {}, off() {}};
  const binding = createBinding(editor, provider, 'main', doc, new Map());
  binding.root.getSharedType().observeDeep((events, transaction) => {
    if (transaction.origin !== binding) syncYjsChangesToLexical(binding, provider, events, false);
  });
  editor.registerUpdateListener(({prevEditorState, editorState, dirtyElements, dirtyLeaves, normalizedNodes, tags}) => {
    syncLexicalUpdateToYjs(binding, provider, prevEditorState, editorState, dirtyElements, dirtyLeaves, normalizedNodes, tags);
  });
  return {editor, binding, provider};
}

const settle = () => new Promise((r) => setTimeout(r, 20));
const out = async (doc, editor) => {
  await settle();
  console.log(JSON.stringify({update: hex(Y.encodeStateAsUpdate(doc)), json: editor.getEditorState().toJSON()}));
};
const [cmd, file] = process.argv.slice(2);

if (cmd === 'gen') {
  const doc = new Y.Doc();
  const {editor} = bound(doc);
  editor.update(() => {
    const r = $getRoot();
    r.clear();
    const p = $createParagraphNode();
    p.setFormat('center');
    p.setIndent(1);
    p.append(
      $createTextNode('hi').toggleFormat('bold').setStyle('color: red'),
      $createTextNode(' there \u{1F600}'),
      $createLineBreakNode(),
      $createLinkNode('http://x', {target: '_blank'}).append($createTextNode('l')),
    );
    r.append(
      p,
      $createHeadingNode('h2').append($createTextNode('H')),
      $createQuoteNode().append($createTextNode('q')),
      $createListNode('check').append($createListItemNode(true).append($createTextNode('a'))),
      $createListNode('number', 3).append(
        $createListItemNode().append($createTextNode('b')),
        $createListItemNode().append($createListNode('bullet').append($createListItemNode().append($createTextNode('inner')))),
      ),
      $createCodeNode('js').append($createTextNode('x=1')),
    );
  }, {discrete: true});
  await out(doc, editor);
} else if (cmd === 'load' || cmd === 'edit') {
  const doc = new Y.Doc();
  const {editor, binding, provider} = bound(doc);
  Y.applyUpdate(doc, unhex(readFileSync(file, 'utf8')));
  await settle();
  if (cmd === 'edit') {
    editor.update(() => {
      const first = $getRoot().getAllTextNodes()[0];
      first.setTextContent(first.getTextContent() + ' js');
    }, {discrete: true});
  }
  await out(doc, editor);
} else {
  console.error('usage: node yjs-interop.mjs gen | load FILE | edit FILE');
  process.exit(2);
}
