// Print the docs stamp of the two manuals' sources: a 64-bit FNV-1a hash of
// every file the books are built from (book.toml, pictures.css, src/ and the
// formula and scripting references they include), in path order. The app's
// test `embedded_manuals_match_their_sources` computes the same hash and
// fails when it differs from crates/app-gui/assets/manuals/docs-stamp.txt,
// that is, when the Markdown changed but scripts/build-manual-pdfs.sh was not
// run. Keep the two in step.
//
//   node scripts/manual-stamp.mjs      (run from the repository root)
import fs from 'node:fs';
import path from 'node:path';

// The sources, relative to the repository root.
export const SOURCES = [
  'docs/formulas.md',
  'docs/guide/book.toml',
  'docs/guide/pictures.css',
  'docs/guide/src',
  'docs/manual/book.toml',
  'docs/manual/pictures.css',
  'docs/manual/src',
  'docs/scripting.md',
];

// Text files are hashed with line feeds only, so a Windows checkout agrees.
const TEXT = new Set(['.md', '.toml', '.css', '.txt', '.js', '.hbs', '.html', '.svg']);

function files(root, rel, out) {
  const full = path.join(root, rel);
  const stat = fs.statSync(full);
  if (stat.isDirectory()) {
    for (const name of fs.readdirSync(full)) {
      // Hidden files (.DS_Store and the like) are not part of the books.
      if (!name.startsWith('.')) files(root, rel + '/' + name, out);
    }
  } else {
    out.push(rel);
  }
  return out;
}

export function stamp(root) {
  const paths = SOURCES.flatMap((s) => files(root, s, [])).sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
  let h = 0xcbf29ce484222325n;
  const feed = (bytes) => {
    for (const b of bytes) {
      h ^= BigInt(b);
      h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
    }
  };
  for (const rel of paths) {
    let data = fs.readFileSync(path.join(root, rel));
    if (TEXT.has(path.extname(rel).toLowerCase())) data = Buffer.from(data.filter((b) => b !== 0x0d));
    const len = Buffer.alloc(8);
    len.writeBigUInt64LE(BigInt(data.length));
    feed(Buffer.from(rel));
    feed([0]);
    feed(len);
    feed(data);
  }
  return h.toString(16).padStart(16, '0');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  console.log(stamp(process.cwd()));
}
