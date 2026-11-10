// Generates the programs' TypeScript clients from their IDLs into
// src/generated. Run `anchor build` in the repo root first, then
// `npm run generate`; commit the output with the program change.
import { readFileSync } from 'node:fs';
import { createFromRoot } from 'codama';
import { rootNodeFromAnchor } from '@codama/nodes-from-anchor';
import { renderVisitor } from '@codama/renderers-js';

for (const program of ['receivables_pool', 'snapshot_roots']) {
  const idl = JSON.parse(readFileSync(new URL(`../../target/idl/${program}.json`, import.meta.url), 'utf8'));
  const codama = createFromRoot(rootNodeFromAnchor(idl));
  const out = new URL(`../src/generated/${program.replace('_', '-')}`, import.meta.url).pathname;
  await codama.accept(renderVisitor(out, { deleteFolderBeforeRendering: true, formatCode: true, generatedFolder: '.', syncPackageJson: false }));
}
