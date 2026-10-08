// Bundle the shim and Lexical into one ES module for `jco componentize`.
//
// Two accommodations for the JavaScript-in-Wasm runtime:
//  * it has no `process`, so Lexical's `process.env.NODE_ENV` switch is fixed to production;
//  * its regex engine lacks Unicode property escapes (\p{L}, \p{N}), which only appear in
//    @lexical/link's auto-link URL matcher, so they are replaced by close approximations.

import {build} from 'esbuild';
import fs from 'node:fs';

const approximateUnicodeProperties = {
  name: 'approximate-unicode-properties',
  setup(b) {
    b.onLoad({filter: /node_modules\/@lexical\/link\/.*\.m?js$/}, (args) => ({
      contents: fs
        .readFileSync(args.path, 'utf8')
        .replaceAll('\\p{L}', 'a-zA-Z\\u00C0-\\uFFFF')
        .replaceAll('\\p{N}', '0-9'),
      loader: 'js',
    }));
  },
};

await build({
  entryPoints: ['src/entry.js'],
  bundle: true,
  format: 'esm',
  platform: 'neutral',
  mainFields: ['module', 'main'],
  outfile: 'dist/entry.mjs',
  plugins: [approximateUnicodeProperties],
  define: {'process.env.NODE_ENV': '"production"'},
  logLevel: 'warning',
});
