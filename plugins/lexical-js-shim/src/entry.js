// The component's export: `lexical:editor/document-plugin`.
import {run} from './shim.js';

export const documentPlugin = {
  run(state, selection, command, payload) {
    return run(state, selection ?? null, command, payload);
  },
};
