// Interop fixtures between Lexical for JavaScript and lexical-core.
//
//   node fixtures.mjs generate   write ../../crates/lexical-core/tests/fixtures/lexical-js.json
//                                with real Lexical's serialization of a document
//   node fixtures.mjs check F    load F (JSON exported by lexical-core) into real Lexical,
//                                fail if it is rejected, print the re-serialized JSON
import {createHeadlessEditor} from '@lexical/headless';
import {$getRoot, $createParagraphNode, $createTextNode, $createLineBreakNode, $createTabNode} from 'lexical';
import {HeadingNode, QuoteNode, $createHeadingNode, $createQuoteNode} from '@lexical/rich-text';
import {ListNode, ListItemNode, $createListNode, $createListItemNode} from '@lexical/list';
import {LinkNode, AutoLinkNode, $createLinkNode} from '@lexical/link';
import {CodeNode, CodeHighlightNode, $createCodeNode} from '@lexical/code';
import {readFileSync, writeFileSync} from 'node:fs';

const nodes = [HeadingNode, QuoteNode, ListNode, ListItemNode, LinkNode, AutoLinkNode, CodeNode, CodeHighlightNode];
const make = () => createHeadlessEditor({nodes, onError: (e) => { throw e; }});

const [cmd, file] = process.argv.slice(2);
if (cmd === 'generate') {
  const e = make();
  e.update(() => {
    const r = $getRoot();
    r.clear();
    const p = $createParagraphNode();
    p.setFormat('center');
    p.setIndent(2);
    p.append(
      $createTextNode('hi').toggleFormat('bold').toggleFormat('italic').setStyle('color: red'),
      $createLineBreakNode(),
      $createTabNode(),
      $createLinkNode('http://x', {target: '_blank', rel: 'noopener', title: 't'}).append($createTextNode('l')),
    );
    const nested = $createListItemNode().append(
      $createListNode('bullet').append($createListItemNode().append($createTextNode('inner'))),
    );
    r.append(
      p,
      $createHeadingNode('h2').append($createTextNode('H')),
      $createQuoteNode().append($createTextNode('q')),
      $createListNode('check').append($createListItemNode(true).append($createTextNode('a'))),
      $createListNode('number', 3).append($createListItemNode().append($createTextNode('b')), nested),
      $createCodeNode('js').append($createTextNode('x=1')),
      $createParagraphNode(),
    );
  }, {discrete: true});
  const out = new URL('../../crates/lexical-core/tests/fixtures/lexical-js.json', import.meta.url);
  writeFileSync(out, JSON.stringify(e.getEditorState().toJSON(), null, 2) + '\n');
} else if (cmd === 'check') {
  const e = make();
  e.setEditorState(e.parseEditorState(readFileSync(file, 'utf8')));
  console.log(JSON.stringify(e.getEditorState().toJSON()));
} else {
  console.error('usage: node fixtures.mjs generate | check FILE');
  process.exit(2);
}
