// Which Lexical for JavaScript plugins this component runs. Edit this file to run others:
// anything that only needs an editor (no DOM) can be registered here exactly as it would
// be in a web page. This list is the only part of the shim that names a plugin.

import {registerRichText, HeadingNode, QuoteNode} from '@lexical/rich-text';
import {ListNode, ListItemNode} from '@lexical/list';
import {LinkNode} from '@lexical/link';
import {CodeNode} from '@lexical/code';
import {registerMarkdownShortcuts, TRANSFORMERS} from '@lexical/markdown';

/** Node classes the editor must know about, as in `createEditor({nodes})`. */
export const nodes = [HeadingNode, QuoteNode, ListNode, ListItemNode, LinkNode, CodeNode];

/** Register every plugin on a fresh headless editor. */
export function register(editor) {
  registerRichText(editor);
  registerMarkdownShortcuts(editor, TRANSFORMERS);
}
